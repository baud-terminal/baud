# G — text encoding, width and grapheme clustering

> Produced by a multi-agent literature and implementation survey: six
> independent lenses, every load-bearing claim then checked by a separate agent
> against its primary source, and every source hash verified again by hand
> before it was pinned. 30 claims survived, 6 were refuted and are not stated
> here as fact, and 28 places where sources or implementations genuinely
> disagree are named rather than smoothed over.
>
> The four sha256 hashes it reported for `unicode-ch3`, `uax11`, `uax29` and
> `ecma35` each reproduced byte-for-byte on re-fetch, which is why the citations
> below are trusted enough to act on. **It found two live C10 violations in
> baud's own decoder**, one of which rendered `E0 80 AF` as `/`.

*Proposed group G of `docs/research/sources/MANIFEST.toml`. Primary sources:
the Unicode Standard 17.0 chapter 3, UAX #11, UAX #29, ECMA-48 §5.3,
ISO/IEC 10646 clause 11/12, and the source of five shipping terminals.*

This note answers a different question from `c-vt-parser.md`. That one covers
the bytes that are *not* text. This one covers the bytes that are: how they
decode, what happens when they do not, how wide the result is, and what a
"character" even is once combining marks and ZWJ exist.

---

## 1. The question

baud decodes UTF-8 into one `Codepoint` per cell and computes width per
codepoint from five hardcoded zero-width ranges and a dozen wide ranges
(`baud-domain/src/quantity.rs:355-383`). Three things follow that have to be
decided rather than inherited: whether that decoder is *conformant* (it is not —
two clause-C10 violations, both one range check away from fixed); whether one
codepoint per cell is *sufficient* (it is not, and 👩‍💻 costs four columns
today instead of two); and whether fixing the second is worth doing **now**,
before a renderer exists.

The two performance rules bound the answer sharply. A per-byte table lookup is
suspect; a per-cell byte is unavailable, because eight is measured. Everything
below is written against that.

---

## 2. What the standards require

### 2.1 UTF-8 well-formedness is nine ranges, not a table

Unicode 17.0 §3.9.3, D92, Table 3-7 defines well-formedness exhaustively:

```
U+0000..U+007F      00..7F
U+0080..U+07FF      C2..DF   80..BF
U+0800..U+0FFF      E0       A0..BF   80..BF
U+1000..U+CFFF      E1..EC   80..BF   80..BF
U+D000..U+D7FF      ED       80..9F   80..BF
U+E000..U+FFFF      EE..EF   80..BF   80..BF
U+10000..U+3FFFF    F0       90..BF   80..BF   80..BF
U+40000..U+FFFFF    F1..F3   80..BF   80..BF   80..BF
U+100000..U+10FFFF  F4       80..8F   80..BF   80..BF
```

"Any UTF-8 byte sequence that does not match the patterns listed in Table 3-7 is
ill-formed." Two mechanisms enforce it, and **both are needed**: the first-byte
column excludes C0–C1 and F5–FF, and the *second*-byte column is narrowed for
exactly four lead bytes — E0, ED, F0, F4. The standard flags this itself: the
exceptions to the `80..BF` pattern "occur only in the second byte of a
sequence." A decoder that implements only the lead-byte half accepts overlongs.
[[source: unicode-ch3]] §3.9.3

That is nine comparisons on the second byte of a multi-byte sequence, and
**nothing at all on the ASCII path**. It is not a per-byte table lookup and it
does not enlarge the working set.

### 2.2 baud violated C10 twice — both fixed in `036a432`

> **Status.** This section is kept in the past tense deliberately. Both bugs
> were live when the survey ran, both were reproduced against the real crate,
> and both are fixed with tests that fail on the parent commit. It is recorded
> rather than deleted because *how* they survived is the reusable part: one was
> invisible to the type that looked like it should catch it, and the other only
> misfired for a single byte class.

C10: "When a process interprets a code unit sequence which purports to be in a
Unicode character encoding form, it shall treat ill-formed code unit sequences
as an error condition and shall not interpret such sequences as characters."

**Bug 1 — overlong 3- and 4-byte sequences decode as real characters.**
`StreamByte::utf8_lead` (`baud-domain/src/decode.rs:164-171`) is:

```rust
0xc2..=0xdf => Some((1, (self.0 & 0x1f) as u32)),
0xe0..=0xef => Some((2, (self.0 & 0x0f) as u32)),
0xf0..=0xf4 => Some((3, (self.0 & 0x07) as u32)),
_ => None,
```

No second-byte constraint exists anywhere in the crate. `utf8_continuation`
(`:172-174`) accepts any `b & 0xc0 == 0x80`, and the only validity gate is
`Codepoint::new` — `v <= 0x10_FFFF && !(0xD800..0xE000).contains(&v)` — which
**structurally cannot see an overlong**, because a non-shortest form yields a
legal scalar value. The verification pass compiled a probe against the real
`baud-core` and read the viewport: `E0 80 AF` → U+002F `/`, `F0 80 80 AF` →
U+002F `/`, `E0 9F 80` → U+07C0. `E0 9F 80` is the Unicode Standard's own named
example of an ill-formed sequence. `E0 80 AF` → `/` is the canonical
path-traversal exploit.

Two-byte overlongs are already rejected (C0/C1 fall outside the `0xc2..=0xdf`
arm), and surrogates and >U+10FFFF are caught by `Codepoint::new` — probed as
`ED A0 80` → U+FFFD and `F4 90 80 80` → U+FFFD. The hole is exactly the missing
second-byte range for `E0` (must be `A0..BF`) and `F0` (must be `90..BF`).

**Bug 2 — error recovery consumes a valid printable-ASCII successor.**
§3.9.5 is explicit that a converter "must not consume the successor bytes as
part of the ill-formed subsequence whenever those successor bytes themselves
constitute part of a well-formed UTF-8 code unit subsequence", and gives
`<C2 41 42>`, which "must not return `<U+FFFD>` or `<U+FFFD, U+0042>`".

baud returns `<U+FFFD, U+FFFD, U+0042>`. The mechanism is readable at
`baud-core/src/parse/machine.rs:225-230`: the `None =>` arm of `State::Utf8`
emits `REPLACEMENT` and then re-feeds the byte with `self.control(b)`. Ground's
dispatch (`:196-209`) has no printable-ASCII path — ASCII is handled by the
scanner and the fast path, not by the state machine — so the re-fed `0x41` is
not `ESC`, not `is_c0()`, not a `utf8_lead()`, and falls into the `else` branch
commented *"stray continuation byte or 0x7f: not text"*, emitting a **second**
U+FFFD. Recovery works correctly for `ESC`, for C0, and for a multi-byte lead
(`C2 E4 B8 AD` → U+FFFD, U+4E2D). It fails precisely for the case the spec
illustrates. Verified at chunk = 1 and chunk = 4096 by the verification pass;
the mechanism is confirmed here by reading the two arms.

The valid byte is not *deleted*, so the UTR#36 token-joining XSS does not
reproduce — but it is consumed as part of the ill-formed subsequence, which is
what §3.9.5 forbids.

### 2.3 The U+FFFD count is *not* a conformance question

§3.9.5, final two paragraphs: an ill-formed subsequence of more than one code
unit "could be treated as a single error or as multiple errors"; for
`<F0 80 80 41>` both `<U+FFFD, U+0041>` and `<U+FFFD, U+FFFD, U+FFFD, U+0041>`
are conformant, "or could take other approaches." **The count is unconstrained.
Only non-consumption of successors, and non-interpretation, are normative.**

§3.9.6 then describes the recommended practice — not a requirement, in the
standard's own words: "Although the Unicode Standard does not require this
practice for conformance" — of replacing each *maximal subpart* (D93b: the
longest subsequence at an unconvertible offset that is either the initial
subsequence of a well-formed sequence, or of length one) with a single U+FFFD.
Chapter 5 §5.22 is where the standard endorses it, as "useful" and "more
secure". [[source: unicode-ch3]] §3.9.5–3.9.6

The practical delta is tiny and Tables 3-8 through 3-11 print it:

| input | maximal-subpart output |
|---|---|
| `C0 AF E0 80 BF F0 81 82 41` | 8× FFFD, 0041 |
| `ED A0 80 ED BF BF ED AF 41` | 8× FFFD, 0041 |
| `F4 91 92 93 FF 41 80 BF 42` | 5× FFFD, 0041, 2× FFFD, 0042 |
| `E1 80 \| E2 \| F0 91 92 \| F1 BF \| 41` | 4× FFFD, 0041 |

"Unless the beginning of an ill-formed subsequence matches the beginning of some
well-formed sequence, this practice replaces almost every byte of an ill-formed
UTF-8 sequence with one U+FFFD." Only a *truncated-but-valid prefix* collapses.
So one-FFFD-per-byte and maximal-subpart agree on overlongs, surrogates,
>U+10FFFF, C0/C1/F5-FF, and lone continuation bytes; they differ only on
Table 3-11's case. A lone `0x80..0x9F` is a maximal subpart of length one, so
baud's single U+FFFD for a stray continuation byte is already correct under
*both* policies — the count was never the problem. And the four tables are four
ready-made deterministic vectors with their expected output printed alongside.

Two disagreements to record rather than resolve:

* **Normativity.** WHATWG Encoding §8.1.1 says "No other behavior is permitted
  per the Encoding Standard"; Unicode says the practice is not required for
  conformance. They prescribe identical *output*. A README claim should say
  "Unicode-conformant" and "WHATWG-identical" as two separate sentences,
  because only the first is a conformance claim.
* **The UTC contradicted itself in 2017.** Consensus 151-C19 (L2/17-168)
  approved moving Best Practices toward *fewer* U+FFFDs for Unicode 11.0;
  152-C21 (L2/17-222) retracted it after objection (L2/17-197). A reader who
  finds L2/17-168 alone will conclude the recommendation changed. It did not.

### 2.4 C1: the 7-bit form is first-class, and ISO 2022 does not reach here

`docs/vt-coverage.md` currently frames 7-bit-only C1 as an exclusion. ECMA-48
5th ed. §5.3 says otherwise. C1 controls are represented "a) in a 7-bit code by
2-character escape sequences of the form ESC Fe … b) in an 8-bit code by bit
combinations from 08/00 to 09/15; **however, when the announcer sequence
ESC 02/00 04/06 according to Standard ECMA-35 is used, the control functions of
the C1 set are represented by ESC Fe sequences as in a 7-bit code.**"
[[source: ecma48]] §5.3

That is a conforming representation in its own right, and the one an 8-bit
environment switches to under an announcer. baud does not *omit* the 8-bit C1
form; it implements the form ECMA-48 specifies for exactly this situation. That
is a stronger sentence than the current one and it strengthens the existing
S7C1T argument.

The frame above it: ISO/IEC 10646 clause 11 (2010 FCD numbering; clause 12 in
the 2020 and 2024 editions, sentence unchanged) states that "Code extension
control functions for the ISO/IEC 2022 code extension techniques (such as
designation escape sequences, single shift, and locking shift) **shall not** be
used with this coded character set." UTF-8 is not a mode inside ISO 2022; it is
what you get by leaving ISO 2022 through ECMA-35 clause 15.4's DOCS door —
`ESC 02/05 F`, with `ESC 02/05 04/00` (no intermediate) reserved as the return.
10646 §12.2 NOTE 2 gives `ESC 02/05 04/07` = `ESC % G` for UTF-8, and §12.5
gives `ESC % @` for the return. [[source: ecma35]] §15.4, [[source: iso10646-fcd]] §11, §12.2, §12.5

Consequence worth stating plainly in `docs/protocol-decisions.md`: SCS, SO/SI,
SS2/SS3 in a UTF-8 stream are **non-conforming by the character-set standard**.
xterm's translation and baud's are *both* extensions beyond the standard, and
so is the `CellFlags::GRAPHICS` provenance baud keeps beside its translated
codepoint; the standards cannot adjudicate between them. Argue SCS against
xterm and ncurses, not against ECMA-35 — ECMA-35 does not reach it. (VTE's own
source comment on keeping DEC Special Graphic: "This is still used by ncurses
:-(".) This paragraph set xterm's translation against baud's "annotation",
from when baud kept the byte and the flag; baud has translated since f297b21.

**Trap:** 10646 §12.2 NOTE 1 deprecates `ESC % / G` (with the 02/15
intermediate) — a *different* sequence from `ESC % G`. A skim concludes `ESC % G`
is deprecated, which is exactly backwards.

---

## 3. What terminals actually do

| | UTF-8 error handling | raw `80..9F` byte | encoded U+0080..U+009F | `ESC % G` / `ESC % @` | cell size / cluster storage |
|---|---|---|---|---|---|
| **xterm** (#411) | default mode emits one value per *sequence*, which may be an out-of-range code point rather than U+FFFD; `utf8Weblike` moves toward the Unicode/WHATWG answer | **man page and source disagree** — see below | `CASE_IGNORE`: neither printed nor acted on (`charproc.c:3315-3336`), unless `allowC1Printable` | gated on `screen->wide_chars && !screen->utf8_always`; `ESC % G` may itself turn wide-char mode on via `ChangeToWide` | n/a |
| **kitty** (0.48.2) | — | — | dropped C1-as-control entirely: "There is no actual advantage to having C1 controls be controls at all, now that UTF-8 encoding is universal" | — | full UAX #29 grapheme segmentation since 0.42.0, **unconditional, no mode gate** |
| **ghostty** (f2d5758) | Hoehrmann decoder; re-dispatches the offending byte after an error, and tests that the retried byte may be an `ESC` that starts a valid sequence | → U+FFFD (lone continuation rejected) | explicitly ignored, citing xterm (`stream.zig:1183`) | — | **8 bytes** (`packed struct(u64)`), 2-bit `content_tag`; cluster suffix in a page-local bitmap allocator |
| **alacritty** (ede2ac14) | `str::from_utf8` + `error_len()`, restart at `valid_bytes + len` — conformant on `<C2 41 42>` | **executed as C1 controls**, recovered from the `from_utf8` error path (vte `lib.rs:635`) | — | — | **24 bytes**; zero-width chars in `ArrayVec<char, 9>` behind `Option<Arc<CellExtra>>` |
| **wezterm** (78cd82d) | `vtparse` feeds one byte at a time to `utf8parse` and maps `InvalidSequence` → U+FFFD **with no re-feed** — the successor byte is lost | executed as C1 from the ground-state transition table before the decoder sees them | re-dispatched as C1 state transitions (`vtparse lib.rs:695-707`) | — | **24 bytes**; `TeenyString` u64 inlines ≤7 UTF-8 bytes *and* caches the double-width bit in the top two bits |
| **foot** (1.21.0) | — | — | — | — | `grapheme-width-method = double-width`: sum codepoint widths, clamp to 2; manual admits it "may break some applications… cursor de-synchronization" |
| **Windows Terminal** (8c0a234f) | — | — | — | — | graphemes by default, gated on `compatibility.textMeasurement` (config, not escape sequence) |

Notes on that table, because it hides real conflicts:

* **xterm contradicts its own documentation on stray continuation bytes.**
  `xterm.man:2404-2408` says a raw `0x80..0x9F` byte becomes U+FFFD "because a
  UTF-8 sequence cannot begin with those bytes." `ptydata.c:98-120`
  (`decodeUtf8`) contains an unconditional lookahead — `else if ((i + 1) <
  length && data->next[i+1] > 0x20 && data->next[i+1] < 0x80) { data->utf_data =
  (IChar) c; }` — that passes the raw byte through whenever the next byte
  *already in the buffer* is printable ASCII. `length` is `data->last -
  data->next`, i.e. unconsumed input. **That makes the result of a malformed
  byte depend on where the read boundary fell**: `9B 41` in one read yields a
  C1 CSI (then dropped by `CASE_IGNORE`); split across two reads it yields
  U+FFFD then a printed `A`. Source is the stronger evidence; the man page is at
  best incomplete. Unresolved without a running xterm and a screen-scrape.
* This is chunk invariance failing in the reference implementation.
  baud's unconditional single U+FFFD per stray continuation byte is
  chunk-invariant and matches Table 3-10. **Do not copy xterm here.** It is also
  a good argument for a property test that feeds deliberately-malformed UTF-8
  across every chunk boundary.
* `EXP_C2_CONTROLS`, the xterm hatch that would let a C1 be honoured in ground
  state, is **defined to 0** in `ptyx.h:882-883` (not undefined) and is wired to
  no configure switch, unlike its sibling `EXP_XMC_GLITCH`. In a default build
  the `CASE_IGNORE` drop is unconditional in parse state.
* **Raw `80..9F` is a genuine three-way split.** baud's "8-bit C1 is unusable by
  construction" matches ghostty and contradicts alacritty and wezterm. That is a
  defensible choice, but it is a choice, and the DECAWM precedent says both
  sides belong in `docs/vt-coverage.md`.
* alacritty's vte deliberately does *not* re-parse after a UTF-8 error ("it's
  easier to just skip it and invalid utf8 is pretty rare anyway"); ghostty
  deliberately does. Both claim maximal-subpart conformance. They differ on
  adversarial input where an invalid sequence is followed by a control byte.

**xterm's `utf8` resource is a four-valued keyword, not a boolean** — `Sres`,
default `"default"`: false(0), true(1), always(2), default(3), other nonzero
treated as 1. Only an *explicitly configured* `always` makes `ESC % G` /
`ESC % @` inert, because `utf8_always` is latched at `charproc.c:10062` before
the `default`→`uAlways` resolution at `:10162`. Mode `?1020` mirrors the live
`screen->utf8_mode`: `do_dec_rqm` answers `MdBool(...)` — 1 or 2 — while
`dpmodes`, `savemodes` and `restoremodes` all list it under "these are readonly,
not saved or restored".

---

## 4. Width and grapheme clustering

### 4.1 There is no standard to conform to

UAX #11 §2 Scope, verbatim: "The East_Asian_Width property is not intended for
use by modern terminal emulators without appropriate tailoring on a case-by-case
basis… does not provide an off-the-shelf solution for all situations."
[[source: uax11]] §2

So there is no primary source that says "a terminal shall do X" for width.
baud's width policy is a **documented choice**, exactly like the DECAWM default
already recorded in `docs/vt-coverage.md`, and should be written the same way:
choice, who else does it, what breaks either way. It is not conformance.

Two further facts that close off the shortcut:

* **EAW has no zero-width value** (N, Na, W, F, H, A). The whole Combining
  Diacritical Marks block is `0300..036F ; A # Mn`, and §5 says Ambiguous
  defaults to narrow, i.e. one cell. §6.2: "nonspacing marks do not possess
  actual advance width. Therefore, even when displaying combining marks, the
  East_Asian_Width property cannot be related to the advance width of these
  characters." An EAW-only width function gives a combining accent one column.
  Zero-width must come from `General_Category` or UAX #29, never from
  `EastAsianWidth.txt`. (UAX #11 never says "column" or "cell"; wide → 2 cells
  is a wcwidth convention layered on top.)
* **Ambiguous is where TUIs live.** 112 of the 128 Box Drawing codepoints and
  20 of the 32 Block Elements are `EAW=A`. Not uniformly: U+2591 LIGHT SHADE is
  Neutral while U+2592 MEDIUM SHADE is Ambiguous, and all ten quadrants
  U+2596..259F are Neutral — no per-block shortcut is correct. UAX #11 resolves
  A from context (language tag, script, font, markup) a monospaced grid does not
  have, so a terminal is permanently in the fallback branch, and the fallback
  (narrow) is the wrong answer for a CJK user whose locale expects wide. This is
  why xterm ships `cjkWidth`, wezterm `treat_east_asian_ambiguous_width_as_wide`,
  Windows Terminal `compatibility.ambiguousWidth`, and iTerm2 a checkbox.

### 4.2 UAX #29 is a small machine with a free conformance suite

The extended grapheme cluster rules (tr29-47 §3.1.1 — an *uncaptioned* table;
"Table 1c" is a different table, Regex Definitions, in §3): GB1–GB5 (sot/eot,
CR×LF, controls), GB6–GB8 (Hangul jamo), GB9/9a/9b (Extend, ZWJ, SpacingMark,
Prepend), GB9c (Indic conjuncts), GB11 (emoji ZWJ), GB12/GB13 (regional
indicator pairs), GB999. GB10 was retired in Unicode 15.1 and no longer exists.
[[source: uax29]] §3.1.1

Thirteen property classes; GB12/GB13 need a parity bit, GB11 a one-bit
"last was Extended_Pictographic", GB9c the InCB linker state. **Three or four
bits of carry state, no allocation** — it fits in `Term` beside the existing
parser state.

`GraphemeBreakTest-17.0.0.txt` is **766 test cases** (764 unique sequences; the
file's own trailer says "Lines: 766"), each an explicit codepoint sequence with
÷/× at every position and the firing rule number named in a trailing comment.
That is 766 deterministic tests with an *external* oracle, each of which points
at the defect when it fails. Note the count is version-specific — 1187 at 15.1,
1093 at 16.0, 766 at 17.0 — so "766" must always be written with "17.0.0"
attached. tr29 §7 also warns the tests "cannot be exhaustive": passing all 766
proves the pair-level rule table is right, not that an implementation is
correct.

### 4.3 What baud gets wrong today, with arithmetic

`Width::of` (`baud-domain/src/quantity.rs:355-383`) zeroes exactly five ranges:

```
0x0300..=0x036f   Combining Diacritical Marks
0x1ab0..=0x1aff   Combining Diacritical Marks Extended
0x20d0..=0x20ff   Combining Diacritical Marks for Symbols
0xfe00..=0xfe0f   Variation Selectors
0x200b..=0x200f   ZWSP, ZWNJ, ZWJ, LRM, RLM
```

Everything else is one or two columns. Working the cases through:

| input | codepoints | baud today | correct | why |
|---|---|---|---|---|
| 👩‍💻 | U+1F469 + U+200D + U+1F4BB | 2 + 0 + 2 = **4** | 2 | one GB11 cluster, one wide cell |
| 👍🏽 | U+1F44D + U+1F3FD | 2 + 2 = **4** | 2 | GB9 Extend; modifier is `EAW=W` in the UCD |
| بّ | U+0628 + U+0651 SHADDA | 1 + 1 = **2** | 1 | Arabic marks are in none of the five ranges |
| हिन्दी matra | U+094D VIRAMA | **1** | 0 | Indic marks likewise |
| é (NFD) | U+0065 + U+0301 | 1 + 0 = **1** | 1 | ✅ Latin NFD happens to be covered |
| 🇯🇵 | U+1F1EF + U+1F1F5 | 1 + 1 = **2** | 2 | ✅ by accident — RI is in no WIDE range |
| U+FEFF mid-stream | ZWNBSP | **1** | 0 | not in the five ranges |

The five ranges cover Latin NFD and the variation selectors and miss Hebrew
points, Arabic marks, Indic matras, Combining Diacritical Marks Supplement
(U+1DC0..1DFF), Combining Half Marks (U+FE20..FE2F), and every `Mn` in the
astral planes. That is a per-codepoint bug, independent of clustering, and it
does not need UAX #29 to fix — it needs `General_Category ∈ {Mn, Me, Cf}` minus
U+00AD, which is Kuhn's two-stage structure and what alacritty gets from
unicode-width 0.2.2.

**~~Two declared knobs that nothing reads.~~ Three quarters of this is fixed,
and the surviving quarter is the interesting one.** As written: `decode.rs:45`
was `pub fn measure(units: impl Iterator<Item = Decoded>, _t: &WidthTable)`,
discarding the table, so `AmbiguousWidth` and `EmojiWidth` were configuration the
code never consulted; `EmojiWidth::Wide` was additionally unreachable by
construction; and `Decoded::Grapheme` was built nowhere.

Checked against the tree rather than re-read:

* **`measure` no longer exists**, under that name or any other.
* **`AmbiguousWidth` is consulted.** `merged.rs:109`'s `width(self, aw:
  AmbiguousWidth)` resolves East Asian Ambiguous through it, which is why the
  merged property table stores a width *code* rather than a resolved `Width` --
  the policy belongs to the embedder and a table that resolved it would take it
  away. That is the opposite of a knob nothing reads.
* **`EmojiWidth` is gone**, so its unreachable variant is moot. `width.rs` keeps
  one field where it carried two, and says so.
* **`Decoded::Grapheme` is still constructed nowhere.** It is matched in two
  places and produced in none: the type names the concept and nothing yields it.
  That one stands, and it is the same failure class the paragraph named -- a
  declared shape with no producer, which reads as capability.

The note is a survey taken at a point in time and its claims are in the present
tense, which is how three of them came to outlive the code. **A dated survey is
still a claim about now unless it says otherwise.**

### 4.4 Where the bytes would come from

The interesting fact is that ghostty holds baud's exact 64-bit budget and
supports clusters anyway:

* **ghostty** — `Cell` is `packed struct(u64)` (a Zig *compile-time* width
  assertion, stronger than a size test), with 48 bits used and `_padding: u16`
  free. A 2-bit `content_tag` value `codepoint_grapheme` marks that the cell's
  single stored codepoint is the *first* of a cluster; the suffix lives in a
  per-page `BitmapAllocator(4 * @sizeOf(u21))` and an
  `AutoOffsetHashMap(Offset(Cell), Offset(u21).Slice)`, both carved out of the
  page's own buffer (`page.zig:291-297`, `1763-1776`). **The tag carries no
  index** — the key is the cell's own offset in the page. Capped at
  `grapheme_max_len = 64`.
* **alacritty** — 24 bytes; `Option<Arc<CellExtra>>` per cell, heap-allocated
  only when needed, and that one slot serves combining marks, underline colour
  *and* OSC 8. `MAX_ZEROWIDTH_CHARS = 9`, doc-commented as a DoS bound;
  overflow is dropped silently via `try_push`. Needs `alloc`.
* **wezterm** — 24 bytes; the *text* is a `TeenyString` u64 inlining ≤7 UTF-8
  bytes when the width is also <3, spilling to a boxed `Vec<u8>` otherwise.
  Needs `alloc`, and a cell can no longer be memcmp'd or reinterpreted as bytes,
  which is the discipline `Run._pad` exists to protect.

Only ghostty's answer is available to baud. And baud needs even less of it than
ghostty does: `Content` already reserves `EMPTY = 0x0011_0000` and
`WIDE_TAIL = 0x0011_0001` above U+10FFFF (`cell.rs:33,36`), so a third sentinel
costs **zero bits and zero bytes**. Do not reach for "spare `CellFlags` bits" —
`LINK_SHIFT = 2` with `NLINK = 128` already spends 7 of the 14 non-flag bits on
the OSC 8 link index.

Both terminals that store clusters bound them (9 and 64). An explicit cap is not
optional here for the same reason the style table and link table evict on
collision. Note ghostty's own chunk size is self-described as "based on vibes" —
sweep it, do not inherit 4.

### 4.5 The protocols, and why there is no consensus to conform to

Four incompatible answers exist to "who owns width", and no two of the six
terminals agree:

* **Mode 2027** (contour-terminal/terminal-unicode-core) — the *terminal* owns
  it and does UAX #29. Christian Parpart, dated 2021-09-06, watermarked "Draft",
  ~90 lines of normative LaTeX, changelog reads "0.1.0 (unreleased) — initial
  draft", and the only edits since 2021 are three spelling fixes. It specifies
  almost nothing beyond "do UAX #29" and says nothing about how to compute a
  *cluster's* width, which is the part that breaks TUIs. Citable; not a width
  specification. kitty refuses it — the number appears nowhere in a full-tree
  grep of v0.48.2, and Kovid Goyal closed #7799 with "I probably wont support
  2027 as I have something better in mind." Windows Terminal reduces it to a
  capability query (below). Rio and foot ship it as a toggle.
* **kitty OSC 66** — the *application* owns it: "this protocol solves the issue
  robustly by removing the co-ordination problem and putting only one actor in
  charge of determining string width." `<OSC> 66 ; metadata ; text <ST>`, text
  ≤4096 bytes, keys `s` (scale 1-7), `w` (width in cells 0-7), `n`/`d`, `v`/`h`.
  The block is `s * w` by `s` cells, so `w` equals a base-cell count only when
  `s = 1` — which is the explicitly blessed subset: "It is possible for a
  terminal to implement only the width part of this spec and ignore the scale
  part… In such cases `s` defaults to 1." **`w` applies to the whole text of one
  escape code**, so an application fixing per-cluster widths emits one OSC 66
  per segment.
* **Unicode TTWG (L2/25-263)** — *neither* owns it; the WG defines measurement
  rules both sides implement, versioned by escape sequence.
* **iTerm2 `OSC 1337 ; UnicodeVersion=push/pop`** — wezterm's approach. A
  one-implementation extension in someone else's OSC namespace. Do not copy.

**Do not manufacture an OSC 66 consensus.** Adoption is real but thin and is
*not* skewed to the width half: kitty implements it fully, contour fully
(v0.7.0: "fractional scales and alignment"), foot **width-only**
(`CHANGELOG` 1.21.0: "Support for kitty's text-sizing protocol (`w`, width,
only), OSC-66"), ghostty has a parser (`src/terminal/osc/parsers/kitty_text_sizing.zig`)
but issue #10333 is open and nothing is rendered, and Windows Terminal, wezterm
and Rio have no `66` in their OSC dispatch tables at all. The `w`-only subset is
the cheapest thing baud could implement and the only one needing no Unicode
tables — and it is the *outlier* in practice, not the interoperable target.

Four unresolved width disagreements baud will hit whatever it picks:

| | one side | other side |
|---|---|---|
| lone regional indicator | UCD: `1F1E6..1F1FF ; N`, UAX #11 ED4 excludes RI from Wide → 1 | kitty: "These all have width 2" |
| emoji modifier U+1F3FB..FF | UCD: `W` (GC=Sk, no combining rule zeroes it) → 👍🏽 = 4 columns | kitty tailors them into its zero-width Marks class |
| VS15 (U+FE0E) | mode 2027: "will NOT change the underlying width" | kitty: decreases the previous cell from 2 to 1 — then documents why its own rule is bad |
| spacing marks (`Mc`) | Wukuyon, from D55 / UAX #44 Table 12: positive advance width, must widen | kovidgoyal: makes cluster width unbounded; Devanagari matras "definitely dont double grapheme width" — deferred as "the most complex and controversial", unresolved on kitty#8533 |

UAX #11 also disagrees with itself here: §2 disclaims terminals, while ED4 was
amended specifically to fold UTS #51 `Emoji_Presentation` into Wide — a change
whose only real consumer is terminal-shaped software.

---

## 5. Recommendations, ranked

**1. ~~Add the second-byte range check.~~ DONE — `036a432`.**
*Decision:* constrain the second byte per Table 3-7 — `E0` → `A0..BF`, `ED` →
`80..9F`, `F0` → `90..BF`, `F4` → `80..8F`.
*Reason:* baud today decodes `E0 80 AF` to `/`. That is a C10 violation and the
canonical path-traversal exploit, and `Codepoint::new` structurally cannot catch
it.
*Source:* [[source: unicode-ch3]] §3.9.3 Table 3-7, D92, and §3.2 C10.
*Cost:* the lead byte already selects an arm; the constraint is a range test on
the *next* byte only. Zero comparisons on the ASCII path, zero on the
`C2..DF`/`E1..EC`/`F1..F3` paths, one on four lead bytes. Neither rule moves:
no extra bytes written, no structure enlarged. **Fix the test first** — a
deterministic case asserting `E0 80 AF` does not render `/`, plus the Table 3-8
and 3-9 vectors verbatim.

**2. ~~Stop consuming the printable-ASCII successor byte.~~ DONE — `036a432`.**
*Decision:* give `machine.rs`'s Ground dispatch a printable-ASCII arm, or make
the `State::Utf8` `None =>` re-feed route through the same path the scanner
uses, so `<C2 41 42>` yields `<U+FFFD, 0041, 0042>`.
*Reason:* §3.9.5 names baud's current `<U+FFFD, U+FFFD, U+0042>` as an output a
conforming process must not produce. Also: it is the only place baud's recovery
differs by *byte class*, which is a chunk-invariance hazard.
*Source:* [[source: unicode-ch3]] §3.9.5.
*Cost:* one branch on the error path, which by construction runs only after a
malformed byte. Nothing on the ingest path. Table 3-10's vector
(`F4 91 92 93 FF 41 80 BF 42`) exercises exactly this and prints the expected
output.

**3. Fix the zero-width set; then wire `WidthTable` into `measure` or delete
the enums.**
*Decision:* replace the five ad-hoc ranges with a `Mn | Me | Cf` (minus U+00AD)
zero-width predicate; then either make `measure` read `_t` or remove
`AmbiguousWidth`/`EmojiWidth`.
*Reason:* `بّ` costs two columns and a Devanagari virama costs one; both
desynchronise the cursor against an `ncurses` peer computing with `wcwidth`.
And a config field nothing reads is the same failure as a mode recorded and
never acted on — CLAUDE.md's own rule.
*Source:* [[source: uax11]] §6.2 for why EAW cannot supply zero;
[[source: uax11]] §5 for the Ambiguous default (narrow when context is
unavailable, which for a terminal is always).
*Cost:* the range list grows. It is consulted once per *non-ASCII* codepoint,
never per byte, so the working-set rule is untouched — but the list must stay a
sorted range table, not a 0x110000-entry array. Ambiguous-narrow is the right
default (tr11 §5, Kuhn, xterm's `cjkWidth` default false); the enum should exist
and be *read* so a CJK-locale user can flip it. Flip cost: unmeasured.

**4. Answer DECRQM `?1020` and `?2027`; record `ESC % G` / `ESC % @`.**
*Decision:* `?1020` → set. `?2027` → `3` (permanently set) once clustering
exists, `0` (not recognized) until then. `DECSET`/`DECRST ?2027` accepted and
ignored. `ESC % G` reclassified as *satisfied* (like S7C1T); `ESC % @` stays
counted in `unsupported`.
*Reason:* baud is UTF-8-only by construction, so `?1020` is truthful and
permanent. `?2027` answering `2` would tell an application the mode exists and
is off, and it would set it and believe it took effect — the exact failure
`docs/vt-coverage.md` already refuses for DECRQM. Windows Terminal reached the
same shape independently: `dpmodes` is a bare `break;` for 2027, and `RequestMode`
answers via `mapPerm` → `DECRPM_PermanentlyEnabled`/`Disabled` from
`compatibility.textMeasurement`.
*Source:* xterm `misc.c:5841-5853` and `charproc.c:7997-8002, 8351-8356,
8799-8804` for the read-only precedent; microsoft/terminal `adaptDispatch.cpp:1891-1892,
1937, 2032-2034` and `DispatchTypes.hpp:548,553-559`.
*Cost:* pure parser work, no hot-path effect. **Named divergence:** xterm
answers `?1020` with `MdBool` — 1 or 2, *not* 3/4 — even though the mode is
read-only. Whether an application distinguishes 1 from 3 here is unmeasured; if
in doubt, match xterm exactly, since xterm defines the mode.
`ESC % G`/`ESC % @` currently reach `machine.rs:330-334`'s
`self.unsupported.inc()`, so they are counted rather than silently dropped —
correct for `ESC % @`, wrong for `ESC % G`, which baud already satisfies. Add
both rows.

**5. Reword the C1 row in `docs/vt-coverage.md`.**
*Decision:* say baud implements the 7-bit `ESC Fe` form that ECMA-48 §5.3(a)
specifies, rather than that it *omits* the 8-bit form; and say the 8-bit C1
refusal follows ghostty and diverges from alacritty and wezterm.
*Source:* [[source: ecma48]] §5.3.
*Cost:* documentation only.

**6. Grapheme clustering: do nothing yet.**
*Decision:* do not implement UAX #29 segmentation or cluster storage now.
*Reason:* there is no renderer and no shaping. Cluster storage buys nothing a
viewport materialiser cannot yet consume, and the width question it answers is
dominated by recommendation 3, which is cheaper and independent. The design is
available when it is needed — ghostty's, which is the only one that fits
`no_std` and 8 bytes — and it costs zero cell bytes because `Content` already
reserves sentinels above U+10FFFF. The side table would key on the cell's
position, be reached only when the sentinel says so (so the ASCII path's working
set is unchanged — the same "branch outside the loop" discipline as
`put_ascii_run`), and must be bounded by construction, with a cap swept rather
than inherited. `GraphemeBreakTest-17.0.0.txt`'s 766 vectors are the acceptance
suite, available now. Cost: **unmeasured**, and there is no honest estimate to
give.
*Also do nothing on OSC 66* — the `w`-only subset is spec-blessed and cheap,
but it is one implementation out of two shipping ones, and implementing it
before there is anything to render reserves cells nothing draws into.

---

## 6. Sources to add to `MANIFEST.toml`

Add `G  text encoding, width and grapheme clustering` to the header comment in
the same commit. Pin **revision-locked** URLs only — never `/master/`, never a
GitHub `/blob/` page (SPA shell, will not hash stably).

| key | title | url | note |
|---|---|---|---|
| `unicode-ch3` | The Unicode Standard 17.0, ch. 3 "Conformance" | `https://www.unicode.org/versions/Unicode17.0.0/core-spec/chapter-3/` | sha256 `69fef5215c021368a1880d91bc59d7935d13a5455f1373eda17b8818d41b7b41`, 404,354 B. **No per-chapter PDF for 17.0** — `ch03.pdf` 404s. Alternative pin: `UnicodeStandard-17.0.pdf`. Fetch with raw `curl`; the summarizer truncates it. |
| `unicode-ch5` | The Unicode Standard 17.0, ch. 5, §5.22 | `https://www.unicode.org/versions/Unicode17.0.0/core-spec/chapter-5/` | where the standard endorses maximal-subpart substitution as "useful", "more secure" |
| `uax11` | UAX #11: East Asian Width, rev. 44 | `https://www.unicode.org/reports/tr11/tr11-44.html` | sha256 `a82167102910152a30e3a3c4984fd5f9b1d8caf47e5d53dc1e1aef050ba4f25e`, 36,530 B. Not `/reports/tr11/` — that floats. |
| `uax29` | UAX #29: Unicode Text Segmentation, rev. 47 | `https://www.unicode.org/reports/tr29/tr29-47.html` | sha256 `130d4e0d3f9da41bf9ad84bf580b3b92edc83886457cbc5a53b432761fd2c6ed`, 138,801 B |
| `eaw-data` | EastAsianWidth-17.0.0.txt | `https://www.unicode.org/Public/17.0.0/ucd/EastAsianWidth.txt` | sha256 `ea7ce50f3444a050333448dffef1cadd9325af55cbb764b4a2280faf52170a33`, 201,595 B |
| `gbtest` | GraphemeBreakTest-17.0.0.txt | `https://www.unicode.org/Public/17.0.0/ucd/auxiliary/GraphemeBreakTest.txt` | sha256 `e2d134d2c52919bace503ebb6a551c1855fe1a1faec18478c78fff254a1793ec`, 126,570 B, 766 cases |
| `ecma35` | ECMA-35, 6th ed. (Dec 1994) | `https://ecma-international.org/wp-content/uploads/ECMA-35_6th_edition_december_1994.pdf` | sha256 `cd812a506f93bf44e0e6058fe423144c4dea8a64fbc96589cf99bcbe32fc1bf4`, 303,826 B. **All content streams are LZW** — `grep` finds nothing; needs a decoder. |
| `iso10646-fcd` | ISO/IEC 10646:2010 (E) **Final Committee Draft** | `http://unicode.org/L2/L2010/10038-fcd10646-main.pdf` | sha256 `ddf5a815ab3ddfb335608a417979eda722d5150a12afd12b2c70d55fe2a24326`, 3,007,065 B. **Must be labelled a draft.** Clause numbers shift in published editions: 11→12, 12.2→13.2, 12.5→13.5. FlateDecode; needs extraction. |
| `mode2027` | Unicode in Terminals (mode 2027 draft) | `https://raw.githubusercontent.com/contour-terminal/terminal-unicode-core/64f53851ceab9a3cf08db4939bcaef75a0899573/spec/terminal-unicode-core.tex` | sha256 `f23237de5dd88ec8fee0c8059a6c979ca2eecc3e4c8fdf8ce4c4d86b7a2e47af`. Draft, ~90 normative lines. |
| `kitty-text-sizing` | kitty text sizing protocol (OSC 66) | `https://raw.githubusercontent.com/kovidgoyal/kitty/v0.48.2/docs/text-sizing-protocol.rst` | sha256 `98300c9d1cab5269ded5b378eb0043bbe40f7ea9e0472e1ddc8b8c8e5b2fda27`, 25,872 B. Pin `kitty/control-codes.h` too — the `.rst` templates the number and never states "66" authoritatively. |
| `xterm-charproc` | xterm `charproc.c` | `https://raw.githubusercontent.com/ThomasDickey/xterm-snapshots/9489b2056ee51fa9dd6a7087483b9b8f85d6a0c4/charproc.c` | sha256 `f3e838a4494c1093d831246a0cb0ed301d1af14e52aa5ab8f66e6a8bb0db63a0`, patch #411 |
| `xterm-ptyx` | xterm `ptyx.h` | same commit path | **required alongside `charproc.c`** — `EXP_C2_CONTROLS = 0` lives here (defined to 0, *not* undefined) |
| `xterm-ptydata` | xterm `ptydata.c` | same commit path | `decodeUtf8` lookahead, v1.172 sha256 `3c2335989c14c950515029bae03ae415d73c2fdc56ab413149718f0e797933db` |
| `ghostty-page` | ghostty `src/terminal/page.zig` | `https://raw.githubusercontent.com/ghostty-org/ghostty/f2d5758f6305867dc36b36293c6165d8152b853e/src/terminal/page.zig` | sha256 `b589082c22ccd3ec771cbbc23a8b6c6f1b582dc33b3ab69f5ffd5962aa3c1991`, 4,462 lines |
| `alacritty-cell` | alacritty `alacritty_terminal/src/term/cell.rs` | `https://raw.githubusercontent.com/alacritty/alacritty/ede2ac144da4dec4c075bfa803aacf3b3739bce6/alacritty_terminal/src/term/cell.rs` | 331 lines |
| `wezterm-cell` | wezterm `wezterm-cell/src/lib.rs` | raw path at commit `78cd82dbba7315814bfbff40e246b8bed4b702e7` | `TeenyString` at L546-559 |
| `wt-adaptdispatch` | microsoft/terminal `src/terminal/adapter/adaptDispatch.cpp` | raw path at commit `8c0a234f056910776e56afa3f8a38d6ddc3db33c` | pin `DispatchTypes.hpp` and `doc/cascadia/profiles.schema.json` (sha256 `f8fb9ede3130cbe81fdda58b82eee329d99ab9c70ed050178bcad76e9ade9769`) alongside |

Note that `MANIFEST.toml`'s `dec-special-graphics` entry said, when this was
written, that no machine-readable mapping was obtained. That was true of the
*DEC-authored* sources read and false as a statement about availability: xterm's
`dec2ucs()`, VTE's `line_drawing_map`, kitty's `charset_translations` and foot's
`vt100_0[]` are all fetchable and hashable, and they disagree on byte `0x5F`.
foot leaves `_`, VTE and alacritty give U+0020, kitty and the Linux console give
U+00A0, and xterm stores 0 and draws, copies and indexes it as `' '`
(`[[source: xterm-charsets-c]]` line 435, `[[source: xterm-screen]]` lines
1893-1894, `[[source: xterm-button]]`). The two statements should not be
conflated in a file whose purpose is making citations checkable.

What holds of the old statement is narrower: **none of the DEC texts read gives
a Unicode codepoint**. The entry is `retrieved`, and `[[source: vt100-ug-ch3]]`
beside it names the set's 32 positions in text, `0x5F` "Blank" first. baud's
codepoints are foot's, and `0x5F`, which foot leaves as `_`, is open:
docs/record.md, "DEC Special Graphics begins at 0x5F, and what it stores there
is the owner's call".

---

## 7. What remains unmeasured or unresolved

1. **Cluster storage cost — unmeasured.** No sweep of a side-table chunk size,
   no measurement of the sentinel branch on the cells path, no figure for what a
   position-keyed side map does to the working set. ghostty's
   `grapheme_chunk_len = 4` is self-described as "based on vibes"; inheriting it
   would be inheriting an unmeasured constant, which this repo does not do.
2. **Ambiguous-width flip cost — unmeasured.** Wiring `WidthTable` into
   `measure` adds a read per non-ASCII codepoint. Expected free; not measured.
3. **The C10 fixes were verified by execution in the verification pass, not
   here.** `cargo` is not on `PATH` in this environment (nix/devenv). The
   source-level facts — no second-byte check anywhere in either crate, and the
   `machine.rs:225-230` re-feed reaching Ground's non-text `else` — are verified
   by reading. Re-run both before anything public quotes the emitted codepoints.
4. **Reset paths are untraced.** Whether `RIS`, `DECSTR` or `Term::new` clears
   `State::Utf8` is unknown; a surviving lead byte would eat the first byte after
   a reset. Not a conformance violation either way (a read boundary is not
   end-of-stream), but it is a chunk-invariance hazard of exactly the class this
   repo has already been bitten by.
5. **xterm's man page versus xterm's source** on stray continuation bytes is
   unresolved. Settling it needs a running xterm and a screen-scrape, or a
   maintainer's confirmation. Source is the stronger evidence and is what the
   table above reports.
6. **The published ISO/IEC 10646 was not read.** Everything here comes from the
   2010 FCD (and, for the prohibition sentence, a 2024 CD where it is verbatim
   identical). Obtaining a published edition would fix the clause numbers;
   the manifest must label the draft as a draft until then.
7. **ISO-IR 196** — the registry entry assigning `ESC 02/05 04/07` to UTF-8 —
   was not retrieved during this pass and is corroborated only by secondary
   compilations plus 10646 §12.2 NOTE 2. Do not cite it as a pinned primary
   source without a live fetch.
8. **wezterm issue #7814** reports ASCII rendering correctly at fractional
   OSC 66 ratios while CJK shows placeholders, which superficially suggests a
   partial wezterm implementation. The OSC dispatch table has no `66`, so it
   cannot be honouring the sequence; the reporter's observation is unexplained.
9. **Font shaping remains the largest unknown in the project**, and nothing in
   this note changes that. Every width figure here is font-independent by
   construction (`DisplayWidth::measure` takes a `WidthTable` and nothing else),
   which is a wire contract with `ncurses` — not a claim that a renderer will
   agree.