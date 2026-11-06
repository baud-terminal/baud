//! Character set designation (`ESC ( ) * + C`) and the locking shifts SO/SI.
//!
//! `[[source: ctlseqs]]`:
//! * `ESC ( C` — "Designate G0 Character Set, VT100, ISO 2022", with `ESC ) C`,
//!   `ESC * C` and `ESC + C` for G1, G2 and G3.
//! * `C = 0` — "DEC Special Character and Line Drawing Set, VT100."
//! * `C = B` — USASCII.
//! * SO — "Switch to Alternate Character Set (Ctrl-N is Shift Out or LS1)."
//! * SI — "Switch to Standard Character Set (Ctrl-O is Shift In or LS0)."
//!
//! **The glyph table is implemented, from `[[source: xterm-charsets]]`'
//! `map_DEC_Spec_Graphic`, and the cell stays marked.** DEC's own tables name
//! every glyph and give no Unicode codepoint (`[[source: vt100-ug-ch3]]`
//! Table 3-9), so the codepoints are xterm's judgement, cited as xterm's, and
//! `[[source: foot-vt100-0]]` makes the same 31 choices
//! (`dec_graphics_is_xterms_and_foots`, in baud-domain). This header said the
//! table was "deliberately not implemented" and that the cell keeps the byte,
//! after both stopped being true.
//!
//! **It is one entry short.** DEC replaces "ASCII codes 137 through 176"
//! (octal), `0x5F..0x7E`, `0x5F` being "Blank"; xterm leaves it `UNDEF`, and
//! foot's table and baud's start at `0x60`. Which codepoint Blank stores is an
//! open question, so nothing here asserts what `0x5F` becomes, and nothing
//! asserts that it stays `_` either.

use crate::support;
use baud_core::*;

fn run(bytes: &[u8]) -> (Vec<PackedCell>, Charset) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    (support::screen(&t), t.charset())
}

fn charset(bytes: &[u8]) -> Charset {
    run(bytes).1
}

fn graphics_at(bytes: &[u8], col: usize) -> bool {
    run(bytes).0[col].flags.is_graphics()
}

// ── designation ─────────────────────────────────────────────────────────────

/// Designating G0 takes effect immediately, because G0 is invoked into GL at
/// power-on.
#[test]
fn designating_g0_takes_effect_at_once() {
    assert_eq!(charset(b""), Charset::Ascii, "power-on is ASCII");
    assert_eq!(charset(b"\x1b(0"), Charset::DecGraphics);
    assert_eq!(charset(b"\x1b(0\x1b(B"), Charset::Ascii);
}

/// Designating G1 does **not** change what is displayed until SO invokes it.
///
/// This is the distinction between designation and invocation, and conflating
/// them makes `ESC ) 0` switch the whole screen to line drawing.
#[test]
fn designating_g1_alone_changes_nothing() {
    assert_eq!(charset(b"\x1b)0"), Charset::Ascii, "still GL = G0 = ASCII");
    assert_eq!(
        charset(b"\x1b)0\x0e"),
        Charset::DecGraphics,
        "SO invokes G1"
    );
}

/// SI puts G0 back.
#[test]
fn si_returns_to_g0() {
    assert_eq!(charset(b"\x1b)0\x0e\x0f"), Charset::Ascii);
}

/// All four slots are designatable and independent.
#[test]
fn all_four_slots_are_independent() {
    // G2 and G3 are designatable; invoking them needs SS2/SS3 or a locking
    // shift baud does not implement, so GL stays on G0.
    assert_eq!(charset(b"\x1b*0"), Charset::Ascii);
    assert_eq!(charset(b"\x1b+0"), Charset::Ascii);
    // Designating G1 does not disturb G0.
    assert_eq!(charset(b"\x1b(B\x1b)0"), Charset::Ascii);
    assert_eq!(charset(b"\x1b(0\x1b)B"), Charset::DecGraphics);
}

/// A designated set is never silently treated as ASCII.
///
/// This asserted `Charset::Other` for `ESC ( <`, back when every set baud did
/// not translate shared one identity. DEC Supplemental has its own now — the
/// point the test was making survives, and is stronger: not translating a set
/// is different from not knowing which set it is.
#[test]
fn an_unhandled_set_is_recorded_not_ignored() {
    assert_eq!(charset(b"\x1b(<"), Charset::Dec(DecSet::Supplemental));
    assert_ne!(charset(b"\x1b(<"), Charset::Ascii, "not silently ASCII");
    assert_ne!(charset(b"\x1b(<"), Charset::DecGraphics, "not line drawing");
}

/// `A` is United Kingdom and is reported as such.
///
/// This asserted `Charset::Ascii` — that UK was "treated as ASCII rather than
/// as unknown" — and the reasoning was that the two differ by one glyph baud
/// did not substitute. Beside the point then, and not true now:
/// `the_national_sets_substitute` puts `£` where UK sends `#`. Reporting the
/// wrong set to an embedder that asked is a lie about what the application
/// designated, whatever the glyph. See `uk_is_designated_not_aliased_to_ascii`.
#[test]
fn british_latin1_is_reported_as_uk() {
    assert_eq!(charset(b"\x1b(A"), Charset::Uk);
    assert_ne!(
        charset(b"\x1b(A"),
        Charset::Other,
        "recognised, not unknown"
    );
}

// ── what reaches the cell ───────────────────────────────────────────────────

/// **A byte under the line-drawing set is translated, and stays marked.**
///
/// This asserted the byte was *untouched*, "the mapping needs a source baud
/// lacks". DEC's tables name each glyph -- `[[source: vt100-ug-ch3]]` Table 3-9
/// calls `q` "Horizontal line - Scan 5" -- and give no codepoint; the codepoints
/// are `[[source: foot-vt100-0]]`'s, cited as foot's rather than as DEC's,
/// because that is whose judgement it is. This said the DEC table "really is
/// pictures", which is true of the VT220 manual's Table 2-4 and not of the
/// VT100 User Guide's.
///
/// Both halves matter. Translating and dropping the flag would lose where the
/// character came from; marking without translating loses the character, and
/// selection and copy hand back `q` where the screen shows `─`.
#[test]
fn a_graphics_byte_is_translated_and_stays_marked() {
    let (g, _) = run(b"\x1b(0q");
    assert!(g[0].flags.is_graphics(), "the cell is still marked");
    assert_eq!(
        g[0].content.as_codepoint().map(|c| c.value()),
        Some(0x2500),
        "`q` is BOX DRAWINGS LIGHT HORIZONTAL"
    );
    assert_ne!(g[0].content, run(b"q").0[0].content, "not the raw byte");
}

/// The whole set, against the pinned table.
#[test]
fn the_line_drawing_set_maps_as_the_pinned_source_says() {
    for (byte, want, name) in [
        (b'j', 0x2518u32, "up and left"),
        (b'k', 0x2510, "down and left"),
        (b'l', 0x250c, "down and right"),
        (b'm', 0x2514, "up and right"),
        (b'n', 0x253c, "vertical and horizontal"),
        (b'q', 0x2500, "horizontal"),
        (b't', 0x251c, "vertical and right"),
        (b'u', 0x2524, "vertical and left"),
        (b'v', 0x2534, "up and horizontal"),
        (b'w', 0x252c, "down and horizontal"),
        (b'x', 0x2502, "vertical"),
        (b'`', 0x25c6, "diamond"),
        (b'a', 0x2592, "checkerboard"),
    ] {
        let mut bytes = b"\x1b(0".to_vec();
        bytes.push(byte);
        let (g, _) = run(&bytes);
        assert_eq!(
            g[0].content.as_codepoint().map(|c| c.value()),
            Some(want),
            "`{}` should be {name}",
            byte as char
        );
    }
}

/// Bytes below the set are ASCII even under it.
///
/// DEC's set is `0x5F..0x7E`; `A` is below it.
#[test]
fn graphics_leaves_the_rest_of_ascii_alone() {
    let (g, _) = run(b"\x1b(0A");
    assert_eq!(
        g[0].content,
        run(b"A").0[0].content,
        "the set replaces 0x5F..0x7E, and `A` is below it"
    );
    assert!(g[0].flags.is_graphics(), "still marked, though");
}

/// ASCII text is not marked.
#[test]
fn ascii_is_not_marked() {
    assert!(!graphics_at(b"q", 0));
    assert!(!graphics_at(b"\x1b(0\x1b(Bq", 0));
}

/// The mark follows the shift, byte by byte, mid-line.
#[test]
fn the_mark_follows_the_shifts_within_a_line() {
    // "a" ASCII, "b" graphics, "c" ASCII again.
    let seq = b"\x1b)0a\x0eb\x0fc";
    assert!(!graphics_at(seq, 0), "a is ASCII");
    assert!(graphics_at(seq, 1), "b is graphics");
    assert!(!graphics_at(seq, 2), "c is ASCII again");
}

/// It applies to the bulk path, not just to single characters.
///
/// Plain ASCII runs never reach the per-codepoint path, so marking only there
/// would leave every real line-drawing run unmarked.
#[test]
fn the_mark_applies_to_bulk_runs() {
    let (g, _) = run(b"\x1b(0qqqqqqqq");
    assert!(
        g[..8].iter().all(|c| c.flags.is_graphics()),
        "every cell of the run must be marked"
    );
}

/// Protection and the charset are independent bits on the same cell.
#[test]
fn protection_and_charset_coexist() {
    let (g, _) = run(b"\x1b(0\x1b[1\"qq");
    assert!(g[0].flags.is_graphics());
    assert!(g[0].is_protected());
    // Turning protection off leaves the charset mark.
    let (h, _) = run(b"\x1b(0\x1b[1\"qq\x1b[0\"qq");
    assert!(h[1].flags.is_graphics());
    assert!(!h[1].is_protected());
}

/// A selective erase still spares protected line-drawing text, and the
/// erasable characters become plain spaces, the line-drawing flag gone with
/// them.
#[test]
fn selective_erase_respects_protected_graphics() {
    let after = run(b"\x1b(0\x1b[1\"qqq\x1b[0\"qww\x1b[1;1H\x1b[?2K").0;
    let kept = run(b"\x1b(0\x1b[1\"qqq\x1b(B\x1b[0\"q  ").0;
    assert_eq!(after, kept);
}

// ── save and restore ────────────────────────────────────────────────────────

/// **DECSC saves the four slots and which one GL reads through, and DECRC
/// brings both back.** `[[source: vt510-decsc]]` lists "Character sets (G0,
/// G1, G2, or G3) currently in GL and GR". `Saved` has held `g` and `gl`
/// since bf8c5cd, and no test held either: a restore that dropped one left
/// the suite green. Here G1 holds DEC graphics and is invoked when the cursor
/// is saved; SI and a redesignation of G1 to ASCII undo both, and DECRC must
/// redo both. GR is held by `decrc_restores_gr`.
#[test]
fn a_cursor_restore_brings_back_the_slots_and_the_shift() {
    let (cells, set) = run(b"\x1b)0\x0e\x1b7\x0f\x1b)B\x1b8q");
    assert_eq!(
        set,
        Charset::DecGraphics,
        "GL reads G1 again, and G1 holds DEC graphics again"
    );
    assert_eq!(
        cells[0].content.as_codepoint().map(|c| c.value()),
        Some(0x2500),
        "q draws the horizontal line"
    );
}

// ── resets and invariance ───────────────────────────────────────────────────

/// RIS returns every slot to ASCII and GL to G0.
#[test]
fn ris_resets_the_charsets() {
    assert_eq!(charset(b"\x1b)0\x0e\x1bc"), Charset::Ascii);
    assert!(!graphics_at(b"\x1b(0\x1bcq", 0));
}

/// Designation and shifting parse the same however the reads are split.
#[test]
fn charset_handling_is_chunk_invariant() {
    let seq = b"\x1b)0abc\x0edef\x0fghi\x1b(0jkl";
    let whole = run(seq).0;
    for chunk in 1..seq.len() {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        let mut i = 0;
        while i < seq.len() {
            let n = chunk.min(seq.len() - i);
            t.write_at(BytePos::ZERO, &seq[i..i + n]);
            t.feed(BytePos::ZERO, ReadLen::from_index(n));
            i += n;
        }
        assert_eq!(support::screen(&t), whole, "differed at chunk {chunk}");
    }
}

/// `ESC ( C` is consumed whole: the final byte must not be printed.
///
/// Asserted through the cursor rather than the cells. This used to compare
/// `a ESC ( 0 b` against `ab` cell for cell, which stopped meaning "the `0`
/// was not printed" the moment `b` began translating to `␉` under the
/// graphics set -- the comparison would have failed for the right reason and
/// the wrong one indistinguishably.
#[test]
fn the_designator_is_not_printed() {
    // The third cell: if the `0` were printed, something would be in it.
    assert_eq!(
        run(b"a\x1b(0b").0[2],
        run(b"ab").0[2],
        "nothing follows the `b` -- the designator was consumed whole"
    );
    assert_eq!(
        run(b"a\x1b(0b").0[0],
        run(b"ab").0[0],
        "and `a` precedes it"
    );
    assert_eq!(run(b"a\x1b(Bb").0, run(b"ab").0, "ASCII is unchanged");
}

/// **`ESC ( A` is United Kingdom, not ASCII.**
///
/// `[[source: ctlseqs]]`: "C = A -> United Kingdom (UK), VT100." It was
/// aliased to `Charset::Ascii`, so an embedder that asked `Term::charset()`
/// after being sent UK was told ASCII — the designation vanished. That is the
/// silent conflation `CSI g` avoids by leaving unhandled parameters inert
/// rather than aliasing them to `0`.
///
/// This said baud records it "and does not substitute the one differing
/// glyph, for the same reason it does not translate DEC Special Graphics".
/// Both are translated now, UK through `[[source: xterm-charsets]]` and the
/// line-drawing set through `[[source: foot-vt100-0]]`. What this pins is
/// unchanged: the designation is *reported as itself*, not as something
/// else.
#[test]
fn uk_is_designated_not_aliased_to_ascii() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let feed = |t: &mut Term, b: &[u8]| {
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index((b).len()));
    };
    assert_eq!(t.charset(), Charset::Ascii, "ASCII by default");
    feed(&mut t, b"\x1b(A");
    assert_eq!(t.charset(), Charset::Uk, "UK is reported as UK");
    feed(&mut t, b"\x1b(B");
    assert_eq!(t.charset(), Charset::Ascii, "and `B` returns to ASCII");
}

/// **Every single-byte charset final `[[source: ctlseqs]]` lists is named.**
///
/// They all collapsed into `Charset::Other`, which meant an embedder could not
/// tell Finnish from a byte baud did not recognise — and "the term *replacement*
/// says that the character set is formed by replacing some of the characters
/// with more useful ones for a given language", so *which* one it is decides
/// which characters change.
///
/// The national ones are translated too, through `[[source: xterm-charsets]]`
/// (`every_national_replacement_renders_as_its_table_says`); this said
/// "recorded, not translated" after the tables landed. The distinction this
/// pins is *named* versus *indistinguishable from garbage*.
#[test]
fn every_listed_single_byte_final_is_named() {
    let cases: &[(&[u8], Charset)] = &[
        (b"\x1b(C", Charset::National(Nrcs::Finnish)),
        (b"\x1b(5", Charset::National(Nrcs::Finnish)),
        (b"\x1b(H", Charset::National(Nrcs::Swedish)),
        (b"\x1b(7", Charset::National(Nrcs::Swedish)),
        (b"\x1b(K", Charset::National(Nrcs::German)),
        (b"\x1b(Q", Charset::National(Nrcs::FrenchCanadian)),
        (b"\x1b(9", Charset::National(Nrcs::FrenchCanadian)),
        (b"\x1b(R", Charset::National(Nrcs::French)),
        (b"\x1b(f", Charset::National(Nrcs::French)),
        (b"\x1b(Y", Charset::National(Nrcs::Italian)),
        (b"\x1b(Z", Charset::National(Nrcs::Spanish)),
        (b"\x1b(4", Charset::National(Nrcs::Dutch)),
        (b"\x1b(=", Charset::National(Nrcs::Swiss)),
        (b"\x1b(`", Charset::National(Nrcs::NorwegianDanish)),
        (b"\x1b(E", Charset::National(Nrcs::NorwegianDanish)),
        (b"\x1b(6", Charset::National(Nrcs::NorwegianDanish)),
        (b"\x1b(<", Charset::Dec(DecSet::Supplemental)),
        (b"\x1b(>", Charset::Dec(DecSet::Technical)),
        (b"\x1b(I", Charset::Dec(DecSet::JisKatakana)),
        (b"\x1b(J", Charset::Dec(DecSet::JisRoman)),
    ];
    for (bytes, want) in cases {
        assert_eq!(charset(bytes), *want, "{:02x?}", bytes);
    }
}

/// The aliases really are aliases, and distinct sets really are distinct.
#[test]
fn charset_aliases_agree_and_distinct_sets_differ() {
    assert_eq!(
        charset(b"\x1b(C"),
        charset(b"\x1b(5"),
        "C and 5 are Finnish"
    );
    assert_ne!(
        charset(b"\x1b(C"),
        charset(b"\x1b(H"),
        "Finnish is not Swedish"
    );
    assert_ne!(
        charset(b"\x1b(K"),
        Charset::Other,
        "German is recognised, not unknown"
    );
}

/// A final byte the specification does not list is still `Other`.
///
/// `x` rather than `!`: `21` is an *intermediate* byte, so `ESC ( !` is not a
/// complete designation at all and never reaches `from_final`.
#[test]
fn an_unlisted_final_is_still_other() {
    assert_eq!(charset(b"\x1b(x"), Charset::Other);
}

/// **Multi-byte charset finals are reachable, and were not.**
///
/// `[[source: ctlseqs]]` lists eleven designations whose final is preceded by a
/// second intermediate — `ESC ( % 6` for Portuguese, `ESC ( " >` for Greek,
/// `ESC ( & 4` for DEC Cyrillic. None could be parsed, and not by decision:
/// `[[source: williams]]`'s `escape_intermediate` *collects* intermediates and
/// baud kept only the last, so the `(` was overwritten by the `%` and the
/// G-slot was gone before the final byte arrived.
#[test]
fn multi_byte_finals_designate_their_set() {
    let cases: &[(&[u8], Charset)] = &[
        (b"\x1b(%6", Charset::National(Nrcs::Portuguese)),
        (b"\x1b(\">", Charset::National(Nrcs::Greek)),
        (b"\x1b(%2", Charset::National(Nrcs::Turkish)),
        (b"\x1b(%=", Charset::National(Nrcs::Hebrew)),
        (b"\x1b(\"4", Charset::Dec(DecSet::Hebrew)),
        (b"\x1b(\"?", Charset::Dec(DecSet::Greek)),
        (b"\x1b(%0", Charset::Dec(DecSet::Turkish)),
        (b"\x1b(%5", Charset::Dec(DecSet::SupplementalGraphics)),
        (b"\x1b(&4", Charset::Dec(DecSet::Cyrillic)),
        (b"\x1b(%3", Charset::Dec(DecSet::Undocumented)),
        (b"\x1b(&5", Charset::Dec(DecSet::Undocumented)),
    ];
    for (bytes, want) in cases {
        assert_eq!(charset(bytes), *want, "{:02x?}", bytes);
    }
}

/// A two-byte final the specification does not list is counted, not recorded.
///
/// Recording it as a set would make it look designated; the whole point of
/// naming the eleven is that a name means the designation was understood.
#[test]
fn an_unlisted_multi_byte_final_is_counted() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    t.write_at(BytePos::ZERO, b"\x1b(%z");
    t.feed(BytePos::ZERO, ReadLen::from_index((b"\x1b(%z").len()));
    assert_eq!(t.charset_unsupported, 1, "counted");
    assert_eq!(
        t.charset(),
        Charset::Other,
        "and not recorded as a real set"
    );
}

/// The single-byte forms still work — two intermediates must not be required.
#[test]
fn single_byte_finals_survive_the_two_intermediate_path() {
    assert_eq!(charset(b"\x1b(0"), Charset::DecGraphics);
    assert_eq!(charset(b"\x1b(B"), Charset::Ascii);
    assert_eq!(charset(b"\x1b(K"), Charset::National(Nrcs::German));
}

/// **The line-drawing table covers `0x60..0x7E`, and the set is one wider.**
///
/// This said the set "differs from ASCII only in `0x60..0x7E`, so 31 entries is
/// the complete mapping rather than a useful subset", and it asserted that `_` is
/// untouched. `[[source: vt100-ug-ch3]]` replaces "ASCII codes 137 through 176"
/// (octal), `0x5F..0x7E`, the first of them "Blank", and so do
/// `[[source: vt102-ug-ch5]]` Table 5-15, `[[source: dec-special-graphics]]` Table
/// 2-4 and `[[source: dec-std-070]]` Appendix A, Table 2. `_` is out of the
/// "outside" list below because asserting it there pinned a departure from every
/// DEC text read as correct. What `0x5F` stores is an open question; the red test
/// for it, on the bulk run, the per-byte path and VT52 `ESC F`, waits for that
/// answer.
#[test]
fn the_line_drawing_table_covers_the_whole_set() {
    for byte in 0x60u8..=0x7e {
        let mut bytes = b"\x1b(0".to_vec();
        bytes.push(byte);
        let (g, _) = run(&bytes);
        let cp = g[0].content.as_codepoint().map(|c| c.value());
        assert!(
            cp.is_some_and(|v| v != u32::from(byte)),
            "0x{byte:02x} `{}` should map to something other than itself",
            byte as char
        );
    }
    // And nothing below the set is touched. Not `_` (0x5F): see above.
    for byte in [b'A', b'Z', b'0', b'9', b' ', b'^'] {
        let mut bytes = b"\x1b(0".to_vec();
        bytes.push(byte);
        let (g, _) = run(&bytes);
        assert_eq!(
            g[0].content.as_codepoint().map(|c| c.value()),
            Some(u32::from(byte)),
            "0x{byte:02x} is below the set's 0x5F..0x7E and is ASCII"
        );
    }
}

// ── national replacement sets, translated ───────────────────────────────────

/// **The national sets substitute, they do not merely get named.**
///
/// Out with the reason "no machine-readable source for the per-language
/// substitutions". `[[source: xterm-charsets]]` has them, as `MAP`/`UNI`
/// macros -- 14 sets, 164 substitutions. 163 come from those macros and are
/// cited as xterm's judgement rather than as DEC's, the same way the
/// line-drawing table is cited as foot's; the other, Turkish 2/1, U+0131, is
/// typed from `[[source: dec-pplv2-pm]]` Figure A-26, which xterm's table
/// lacks.
///
/// **It pins note AF's Q-DECNRCM, (c).** It designates UK, German and French
/// with no `CSI ? 42 h`: the national sets act outside national mode, as
/// `[[source: dec-std-070]]`'s SCS algorithm has it (p. 5-121, no mode
/// test), and departing from the VT220 and VT420 manuals' gate (note AF
/// §2.10). This said the ungated sets departed from every DEC text read,
/// before STD 070's algorithms were read.
#[test]
fn the_national_sets_substitute() {
    // The canonical one: `#` is `£` in the United Kingdom set.
    let (g, _) = run(b"\x1b(A#");
    assert_eq!(
        g[0].content.as_codepoint().map(|c| c.value()),
        Some(0x00a3),
        "UK maps 0x23 to POUND SIGN"
    );
    // German: `[` is `Ä`, `~` is `ß`.
    let (g, _) = run(b"\x1b(K[");
    assert_eq!(
        g[0].content.as_codepoint().map(|c| c.value()),
        Some(0x00c4),
        "German maps 0x5B to A WITH DIAERESIS"
    );
    // French: `#` is `£` too, but `@` is `à`.
    let (g, _) = run(b"\x1b(R@");
    assert_eq!(
        g[0].content.as_codepoint().map(|c| c.value()),
        Some(0x00e0),
        "French maps 0x40 to A WITH GRAVE"
    );
}

/// **DECNRCM is recognised, and with no keyboard dialect it is ignored**
/// (note AF's Q-DECNRCM, (c)): `[[source: dec-std-070]]`'s DECNRCM note 3,
/// p. 5-126, "This sequence is ignored when the North American or Dutch
/// Keyboard Dialect is selected", and baud has no dialect setting, its
/// keyboard a North American one, for which 070-6 p. 6-58 has "7-bit
/// Characters mode is not available". So `CSI ? 42 h` and `l` change
/// nothing -- no re-initialisation of G0-G3 (note 1), German still in G0
/// -- and are not counted as a mode baud lacks, at Level 4 and at Level 1,
/// DECNRCM's "1x". `DECRQM ?42` answers `0` at Level 4: "DEC Private
/// modes that cannot be set or reset from the host will report as unknown"
/// (p. 5-193).
#[test]
fn decnrcm_is_recognised_and_ignored_with_no_keyboard_dialect() {
    // DECRQM is not recognised at Level 1, so it answers nothing there
    // (the DECSCL row).
    for (level, prefix, rqm) in [
        ("Level 4", b"".as_slice(), b"\x1b[?42;0$y".as_slice()),
        ("Level 1", b"\x1b[61\"p", b""),
    ] {
        for mode in [b"\x1b[?42h".as_slice(), b"\x1b[?42l"] {
            let ws = support::CORPUS;
            let mut s = Owned::new(&ws);
            let mut t = Term::new(s.storage(), &ws);
            let program = [prefix, b"\x1b(K", mode, b"[\x1b[?42$p"].concat();
            support::feed(&mut t, &program);
            let what = format!("{level}, {mode:?}");
            assert_eq!(t.unsupported_param, 0, "{what}: not counted");
            assert_eq!(
                support::screen(&t)[0]
                    .content
                    .as_codepoint()
                    .map(|c| c.value()),
                Some(0x00c4),
                "{what}: German stays in G0"
            );
            assert_eq!(t.reply(), rqm, "{what}: DECRQM");
        }
    }
    // The positive control: a private mode baud has no meaning for is
    // counted, so the count above can fire.
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, b"\x1b[?9999h");
    assert_eq!(t.unsupported_param, 1, "?9999 is counted");
}

/// A byte the set does not replace is itself.
#[test]
fn an_unreplaced_byte_is_ascii_under_a_national_set() {
    let (g, _) = run(b"\x1b(Kx");
    assert_eq!(
        g[0].content.as_codepoint().map(|c| c.value()),
        Some(u32::from(b'x')),
        "German replaces a handful of positions, not the alphabet"
    );
}

/// **A cell written under ungated German keeps its glyph when G0 is
/// designated again.**
///
/// The `[` written under `ESC ( K` is U+00C4 in the cell, and it is still
/// U+00C4 after `ESC ( B` and twelve more writes on the same row under ASCII:
/// the cell holds the codepoint the set translated the byte to when it was
/// written, so a later designation has nothing to translate again.
///
/// This was `a_promoted_run_keeps_the_set_it_was_written_under`, documented as
/// proving that `Run::charset` carried the set through `OpMode::Runs`'
/// promotion. Both were `baud-core2`'s. Nothing in this core builds a run or
/// reads that field, so the name and the doc described a mechanism the test
/// never reached (note AF §3.8).
///
/// **It pins note AF's Q-DECNRCM, (c).** It designates German with no
/// `CSI ? 42 h`, as the national sets act whenever designated; under (b),
/// the manuals' gate, German would not act and the test would fail.
#[test]
fn a_cell_written_under_ungated_german_keeps_its_glyph_when_g0_is_designated_again() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let feed = |t: &mut Term, b: &[u8]| {
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index((b).len()));
    };
    // Write `[` under German at 1;1, designate ASCII into G0, then write a `z`
    // at each of columns 2-13 of the same row under ASCII.
    //
    // The twelve are real writes, not cursor moves, so the row is written
    // again after the set changed: a set applied when the row is read, or to
    // the row as a whole, rather than to each cell as it is written, would
    // turn column 1 back into `[`. Twelve is `baud-core2`'s number, where a
    // ninth run on a line promoted it; any write to the row would do here, and
    // the count is kept so the test sends the bytes it always has.
    feed(&mut t, b"\x1b(K[");
    feed(&mut t, b"\x1b(B");
    for col in 2..14u16 {
        let mut b = b"\x1b[1;".to_vec();
        b.extend_from_slice(col.to_string().as_bytes());
        b.extend_from_slice(b"Hz");
        feed(&mut t, &b);
    }
    let first = t.row(Row::from_index(0)).expect("a row on the grid")[0]
        .content
        .as_codepoint()
        .map(|c| c.value());
    assert_eq!(
        first,
        Some(0x00c4),
        "the `[` written under German is still A WITH DIAERESIS"
    );
}

// ── translation is a property of the byte, not of the route ─────────────────

/// **A designated set applies however the byte reached the cell.**
///
/// `baud-core2`'s `put_run` translated and its `put_glyph` did not, so the
/// same byte drew two different glyphs depending on whether the scanner had
/// taken it into a run.
/// `[[source: unicode-ch3]]` § 3.9.5 names the case that makes the per-byte
/// route reachable in ordinary input: after an ill-formed sequence the
/// successor byte "must not be consumed", so it is re-dispatched one at a time
/// -- and with DEC Special Graphics in force it was written untranslated.
#[test]
fn ascii_recovered_after_a_bad_sequence_is_still_translated() {
    // `ESC ( 0` designates DEC graphics; `C2` opens a two-byte sequence that
    // `71` cannot continue, so the `q` is re-dispatched from ground.
    let run_of_q = run(b"\x1b(0qq").0;
    let recovered = run(b"\x1b(0\xc2qq").0;
    // The replacement lands first, then the two `q`s -- so compare the pair
    // that follows it against the pair written as an ordinary run.
    assert_eq!(
        (recovered[1].content, recovered[2].content),
        (run_of_q[0].content, run_of_q[1].content),
        "a byte recovered after a malformed sequence must translate like any other"
    );
}

/// **REP repeats the character the cell shows, not the byte that produced it.**
///
/// `baud-core2`'s `put_run` recorded `Codepoint::from_ascii(b)` as the last
/// graphic character, so `ESC ( 0 q CSI 2 b` drew the line-drawing glyph once
/// and a literal `q` twice -- the same character spelled two ways in adjacent
/// cells.
#[test]
fn rep_repeats_the_designated_glyph_not_the_raw_byte() {
    let v = run(b"\x1b(0q\x1b[2b").0;
    assert_eq!(v[0].content, v[1].content, "first repeat");
    assert_eq!(v[0].content, v[2].content, "second repeat");
}

/// **Every national replacement actually reaches the screen.**
///
/// baud-domain's `nrcs_source.rs` checks the *tables* against
/// `[[source: xterm-charsets]]`, and `every_listed_single_byte_final_is_named`
/// checks the *designators*. Nothing checked the composition: the write path
/// could ignore the tables entirely and both of those would stay green, because
/// one reads the compiled tables and the other reads `Term::charset()` --
/// neither looks at a cell.
///
/// That is the distinction this project keeps paying for. A test that counts
/// what an operation did cannot see it being done to the wrong object, and a
/// test that validates a table cannot see the table going unread. So this one
/// drives bytes through `Term` and asserts the **rendered codepoint**, for every
/// pair of every set that a designator can reach.
///
/// The expectations come from `nrcs_table` rather than from a list here, so the
/// day the pinned source changes, this test changes with it instead of
/// disagreeing with it.
#[test]
fn every_national_replacement_renders_as_its_table_says() {
    // Designators, discovered rather than transcribed: every single-byte final
    // and the two-byte forms, kept when the parser names a national set.
    let mut designators: Vec<(Vec<u8>, Nrcs)> = Vec::new();
    for b in 0x20u8..=0x7e {
        let seq = vec![0x1b, b'(', b];
        if let Charset::National(n) = charset(&seq) {
            designators.push((seq, n));
        }
    }
    for i in [b'"', b'%', b'&'] {
        for f in 0x20u8..=0x7e {
            let seq = vec![0x1b, b'(', i, f];
            if let Charset::National(n) = charset(&seq) {
                designators.push((seq, n));
            }
        }
    }

    // Positive control: the discovery must find the sets, or every assertion
    // below is unreached and the test passes having checked nothing.
    let found: std::collections::BTreeSet<u8> =
        designators.iter().map(|(_, n)| n.to_u8()).collect();
    assert!(
        found.len() >= 12,
        "only {} national sets are reachable by a designator -- the discovery \
         stopped matching",
        found.len()
    );

    let mut checked = 0usize;
    for (seq, set) in &designators {
        for (byte, want) in baud_domain::nrcs_table(*set) {
            let mut bytes = seq.clone();
            bytes.push(*byte);
            let cells = run(&bytes).0;
            let got = cells[0]
                .content
                .as_codepoint()
                .map(|c| c.value())
                .unwrap_or(0);
            assert_eq!(
                got, *want,
                "{set:?}: byte {byte:#04x} must render U+{want:04X}, not U+{got:04X}"
            );
            checked += 1;
        }
    }
    assert!(checked >= 150, "only {checked} replacements checked");
}

/// What a program leaves when fed in these reads: the first row's first
/// three codepoints, the cursor, the title, and whether a reply was queued.
fn left_by(reads: &[&[u8]]) -> ([u32; 3], (Row, Col), Vec<u8>, bool) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    for r in reads {
        t.write_at(BytePos::ZERO, r);
        t.feed(BytePos::ZERO, ReadLen::from_index(r.len()));
    }
    let cells = support::screen(&t);
    let cp = |i: usize| cells[i].content.as_codepoint().map_or(0, |c| c.value());
    (
        [cp(0), cp(1), cp(2)],
        t.cursor(),
        t.title().to_vec(),
        !t.reply().is_empty(),
    )
}

/// **After an intermediate, `P X [ ] ^ _` are finals, at every read size and
/// every split.**
///
/// `[[source: williams]]`'s `escape intermediate` dispatches `30-7E`
/// (`30-7E / esc_dispatch`, 224): `escape` alone opens a `CSI` on `5B`, an
/// `OSC` on `5D`, a DCS on `50` and the other strings on `58 5E 5F` (222,
/// 226, 232, 233). baud's one `Escape` node was both states, so it opened
/// them after an intermediate too, and `fast_esc`, which reads `ESC
/// <intermediate> <final>` whole, did not: `ESC ( ]` read whole designated a
/// set and drew what followed, and split before the `]` opened an `OSC` that
/// took it. After two intermediates `fast_esc` declines and the graph opened
/// the string in every read.
///
/// Each designation is followed by `AB`, which must be drawn at columns 0
/// and 1 whatever the read size, under both encodings: a `CSI` opened would
/// take the `A` for its final (CUU), and a string would take both.
#[test]
fn after_an_intermediate_the_introducer_bytes_are_finals_at_every_read_size() {
    let want = [u32::from(b'A'), u32::from(b'B'), 0];
    for (enc, pre) in [("UTF-8", b"".as_slice()), ("ISO 8859-1", b"\x1b%@")] {
        for inter in [b"(".as_slice(), b" ", b"#", b"%", b"(%"] {
            for f in [b'P', b'X', b'[', b']', b'^', b'_'] {
                let prog = [pre, b"\x1b", inter, &[f], b"AB"].concat();
                let at = format!("{enc}: {}", prog.escape_ascii());
                let whole = left_by(&[&prog]);
                assert_eq!(whole.0, want, "{at}, one read: AB drawn");
                assert_eq!(whole.1, (Row::ZERO, Col::at_index(2)), "{at}, one read");
                assert_eq!(whole.2, b"", "{at}, one read: no title");
                for size in 1..prog.len() {
                    let reads: Vec<&[u8]> = prog.chunks(size).collect();
                    assert_eq!(left_by(&reads), whole, "{at}, read size {size}");
                }
                for cut in 1..prog.len() {
                    let (a, b) = prog.split_at(cut);
                    assert_eq!(left_by(&[a, b]), whole, "{at}, split at {cut}");
                }
            }
        }
    }
    // Positive control: with no intermediate the same bytes do open, and `AB`
    // is not drawn as text.
    let osc = left_by(&[b"\x1b]AB"]);
    assert_eq!(osc.1, (Row::ZERO, Col::ZERO), "ESC ] opens an OSC");
}

// ── the DEC 94-character sets: Supplemental, JIS-Katakana, JIS-Roman ───────

/// Each translated DEC set under each designation that reaches it in GL: G0
/// directly, and G1 through `SO`. `<` and `% 5` both name DEC Supplemental:
/// `<` in `[[source: vt220-rm-ch4]]` Table 4-4, `% 5` in
/// `[[source: dec-std-070]]` p. 5-117.
const DEC_DESIGNATIONS: [(&[u8], DecSet); 6] = [
    (b"\x1b(<", DecSet::Supplemental),
    (b"\x1b(%5", DecSet::SupplementalGraphics),
    (b"\x1b)%5\x0e", DecSet::SupplementalGraphics),
    (b"\x1b(I", DecSet::JisKatakana),
    (b"\x1b)I\x0e", DecSet::JisKatakana),
    (b"\x1b(J", DecSet::JisRoman),
];

fn cp(c: PackedCell) -> u32 {
    c.content.as_codepoint().map_or(0, |c| c.value())
}

/// **Every position of every translated DEC set reaches the screen as its
/// table says, on both write paths.**
///
/// A run of one or two bytes is written a character at a time through
/// `PackedCell::translate`, and a longer one in bulk through
/// `put_ascii_run`; each translates through `with_charset`, and a path that
/// skipped it would draw the byte. So each position is sent once alone and
/// once as a run of eight. The expectations are the tables, which
/// `baud-domain/tests/dec_sets.rs` holds to the DEC texts; this holds the
/// write path to the tables.
#[test]
fn every_dec_set_position_renders_as_its_table_says_on_both_paths() {
    let mut checked = 0;
    for (designate, set) in DEC_DESIGNATIONS {
        let table = set.table().expect("translated");
        for byte in 0x21u8..=0x7e {
            let want = table[usize::from(byte - 0x21)].value();
            let one = run(&[designate, &[byte]].concat()).0;
            assert_eq!(
                cp(one[0]),
                want,
                "{set:?} via {designate:?}: 0x{byte:02x} alone"
            );
            let eight = run(&[designate, &[byte; 8]].concat()).0;
            for (i, c) in eight[..8].iter().enumerate() {
                assert_eq!(
                    cp(*c),
                    want,
                    "{set:?} via {designate:?}: 0x{byte:02x}, cell {i} of a run"
                );
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 6 * 94);
}

/// **What each set draws, spelled out**, so the write path is held to the
/// DEC texts and not only to tables a generator wrote.
///
/// * DEC Supplemental 5/15 is ß (Table 2-3), where xterm draws `_`; 2/1 is ¡;
///   2/4 is reserved and draws the error character U+2426.
/// * JIS-Katakana 3/1 is U+FF71 HALFWIDTH KATAKANA LETTER A, JIS X 0201's
///   0xB1; 6/0 is reserved.
/// * JIS-Roman 5/12 is the Yen sign, `[[source: vt382-rm]]` §2.4.5, and every
///   other letter is ASCII.
#[test]
fn each_dec_set_draws_what_its_dec_text_says() {
    let first = |b: &[u8]| cp(run(b).0[0]);
    assert_eq!(first(b"\x1b(%5_"), 0x00df, "DEC Supplemental 5/15 is ß");
    assert_eq!(first(b"\x1b(<_"), 0x00df, "and under `<`");
    assert_eq!(first(b"\x1b(%5!"), 0x00a1, "2/1 is ¡");
    assert_eq!(
        first(b"\x1b(%5$"),
        0x2426,
        "2/4 is reserved: the error character"
    );
    assert_eq!(
        first(b"\x1b(I1"),
        0xff71,
        "JIS-Katakana 3/1 is HALFWIDTH KATAKANA LETTER A"
    );
    assert_eq!(first(b"\x1b(I`"), 0x2426, "JIS-Katakana 6/0 is reserved");
    assert_eq!(first(b"\x1b(J\\"), 0x00a5, "JIS-Roman 5/12 is YEN SIGN");
    assert_eq!(first(b"\x1b(Ja"), u32::from(b'a'), "JIS-Roman 6/1 is ASCII");
    // Positive control: under ASCII the same bytes draw themselves.
    assert_eq!(first(b"_"), u32::from(b'_'));
}

/// **SP is a space under every DEC set.** It is not one of a 94-character
/// set's graphic characters: "As a graphic character it has the visual
/// representation of the absence of a graphic symbol"
/// (`[[source: dec-std-070]]` 070-3 §3.4.1), so no table indexes it, alone or in a run.
#[test]
fn space_is_a_space_under_every_dec_set() {
    for (designate, set) in DEC_DESIGNATIONS {
        for n in [1, 8] {
            let cells = run(&[designate, &vec![b' '; n][..], b"x"].concat()).0;
            for c in &cells[..n] {
                assert_eq!(cp(*c), 0x20, "{set:?} via {designate:?}, run of {n}");
            }
        }
    }
}

/// **A designation and the text after it draw the same at every read size
/// and every split**: `ESC ( % 5`, a run across both paths, a return to ASCII,
/// and a G1 designation invoked by `SO` and left by `SI`.
#[test]
fn a_dec_set_designation_is_chunk_invariant() {
    let prog: &[u8] = b"\x1b(%5_!$\x1b(Bab\x1b)I\x0e1`\x0fc\x1b(J\\\\\\";
    let whole = left_by(&[prog]);
    // Positive control: the program does draw DEC Supplemental, which the
    // splits are then compared against.
    assert_eq!(whole.0, [0x00df, 0x00a1, 0x2426], "one read");
    let screen = |reads: &[&[u8]]| {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        for r in reads {
            support::feed(&mut t, r);
        }
        support::screen(&t)[..13]
            .iter()
            .map(|c| cp(*c))
            .collect::<Vec<_>>()
    };
    let want = screen(&[prog]);
    assert_eq!(
        want,
        [
            0xdf, 0xa1, 0x2426, 0x61, 0x62, 0xff71, 0x2426, 0x63, 0xa5, 0xa5, 0xa5, 0, 0
        ],
        "one read, the whole program"
    );
    for size in 1..prog.len() {
        let reads: Vec<&[u8]> = prog.chunks(size).collect();
        assert_eq!(screen(&reads), want, "read size {size}");
    }
    for cut in 1..prog.len() {
        let (a, b) = prog.split_at(cut);
        assert_eq!(screen(&[a, b]), want, "split at {cut}");
    }
}

/// **REP repeats the DEC Supplemental character, not its byte**, from either
/// write path: after a lone `_` (per character) and after a run (in bulk).
#[test]
fn rep_repeats_a_dec_set_character() {
    let v = run(b"\x1b(%5_\x1b[2b").0;
    assert_eq!([cp(v[0]), cp(v[1]), cp(v[2])], [0xdf; 3], "after one byte");
    let v = run(b"\x1b(%5!!!_\x1b[2b").0;
    assert_eq!([cp(v[3]), cp(v[4]), cp(v[5])], [0xdf; 3], "after a run");
}

/// **A byte recovered after an ill-formed UTF-8 sequence translates too**:
/// `C2` opens a sequence `_` cannot continue, the replacement lands, and the
/// `_`s are re-dispatched one at a time (`[[source: unicode-ch3]]` § 3.9.5).
#[test]
fn ascii_recovered_after_a_bad_sequence_translates_under_a_dec_set() {
    let v = run(b"\x1b(%5\xc2__").0;
    assert_eq!([cp(v[1]), cp(v[2])], [0xdf, 0xdf]);
}

/// **DECRC brings DEC Supplemental back**: DECSC saves G0-G3 and GL
/// (`a_cursor_restore_brings_back_the_slots_and_the_shift`), so a set saved
/// and restored draws again.
#[test]
fn decrc_restores_a_dec_set() {
    let v = run(b"\x1b(%5\x1b7\x1b(B_").0;
    assert_eq!(cp(v[0]), u32::from(b'_'), "ASCII between save and restore");
    let v = run(b"\x1b(%5\x1b7\x1b(B\x1b8_").0;
    assert_eq!(cp(v[0]), 0xdf, "DEC Supplemental after restore");
}

/// **A 96-set intermediate does not designate a 94-set into G0**: `ESC - % 5`
/// and `ESC . <` name G1 and G2 as 96-character slots
/// (`[[source: dec-std-070]]` p. 5-117), which baud counts and does not act on (note AE
/// Q3), so G0 stays ASCII and `_` is `_`.
#[test]
fn a_96_set_intermediate_designates_no_dec_set() {
    for prog in [b"\x1b-%5_".as_slice(), b"\x1b.<_", b"\x1b/I_"] {
        let (cells, set) = run(prog);
        assert_eq!(set, Charset::Ascii, "{}", prog.escape_ascii());
        assert_eq!(cp(cells[0]), u32::from(b'_'), "{}", prog.escape_ascii());
    }
}
