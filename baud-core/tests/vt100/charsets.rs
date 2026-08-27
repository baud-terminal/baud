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
//! **The glyph table is deliberately not implemented, and that is a sourcing
//! decision.** ctlseqs names the set but gives no byte-to-glyph mapping, and
//! `MANIFEST.toml` records `dec-special-graphics` as `gated` -- vt100.net
//! publishes the table as a figure, so no claim may rest on it. baud therefore
//! keeps the byte the application sent and marks the cell, which loses nothing:
//! a renderer holding the table has everything it needs, and one that does not
//! can say so. Translating to a guessed codepoint would be unrecoverable.

use baud_core::*;

fn run(bytes: &[u8]) -> (Vec<PackedCell>, Charset) {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let mut i = 0;
    while i < bytes.len() {
        let n = READ_CHUNK.min(bytes.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(&bytes[i..i + n]));
        t.feed(base, f.len());
        t.commit_read(f.len());
        i += n;
    }
    (t.viewport().to_vec(), t.charset())
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

/// A set baud does not distinguish is recorded as `Other`, not silently
/// treated as ASCII.
#[test]
fn an_unhandled_set_is_recorded_not_ignored() {
    assert_eq!(charset(b"\x1b(<"), Charset::Other, "DEC Supplemental");
    // and it is not the line-drawing set
    assert_ne!(charset(b"\x1b(<"), Charset::DecGraphics);
}

/// `A` is United Kingdom and is reported as such.
///
/// This asserted `Charset::Ascii` — that UK was "treated as ASCII rather than
/// as unknown" — and the reasoning was that the two differ by one glyph baud
/// does not substitute. True, and beside the point: not substituting a glyph is
/// a *rendering* decision, while reporting the wrong set to an embedder that
/// asked is a lie about what the application designated. See
/// `uk_is_designated_not_aliased_to_ascii`.
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

/// A byte written under the line-drawing set is marked, and keeps its byte.
#[test]
fn a_graphics_byte_is_marked_and_unchanged() {
    let (g, _) = run(b"\x1b(0q");
    assert!(g[0].flags.is_graphics(), "the cell must be marked");
    assert_eq!(
        g[0].content,
        run(b"q").0[0].content,
        "the byte itself is untouched -- the mapping needs a source baud lacks"
    );
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

/// A selective erase still spares protected line-drawing text.
#[test]
fn selective_erase_respects_protected_graphics() {
    let after = run(b"\x1b(0\x1b[1\"qqq\x1b[0\"qww\x1b[1;1H\x1b[?2K").0;
    let kept = run(b"\x1b(0\x1b[1\"qqq").0;
    assert_eq!(after, kept);
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
        let mut s = Storage::boxed();
        let mut t = Term::new(&mut s, OpMode::Computed);
        let mut i = 0;
        while i < seq.len() {
            let n = chunk.min(seq.len() - i);
            let mut slot = t.read_slot();
            let base = slot.base();
            let f = slot.write(StreamByte::slice_from(&seq[i..i + n]));
            t.feed(base, f.len());
            t.commit_read(f.len());
            i += n;
        }
        assert_eq!(t.viewport().to_vec(), whole, "differed at chunk {chunk}");
    }
}

/// `ESC ( C` is consumed whole: the final byte must not be printed.
#[test]
fn the_designator_is_not_printed() {
    assert_eq!(run(b"a\x1b(0b").0[1].content, run(b"ab").0[1].content);
    assert_eq!(run(b"a\x1b(Bb").0, run(b"ab").0);
}

/// **`ESC ( A` is United Kingdom, not ASCII.**
///
/// `[[source: ctlseqs]]`: "C = A -> United Kingdom (UK), VT100." It was
/// aliased to `Charset::Ascii`, so an embedder that asked `Term::charset()`
/// after being sent UK was told ASCII — the designation vanished. That is the
/// silent conflation `CSI g` avoids by leaving unhandled parameters inert
/// rather than aliasing them to `0`.
///
/// baud records it and does not substitute the one differing glyph, for the
/// same reason it does not translate DEC Special Graphics: `MANIFEST.toml`
/// carries no machine-readable DEC source for either mapping. The distinction
/// that matters is *recorded but not translated* versus *reported as something
/// else*.
#[test]
fn uk_is_designated_not_aliased_to_ascii() {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let feed = |t: &mut Term, b: &[u8]| {
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(b));
        t.feed(base, f.len());
        t.commit_read(f.len());
    };
    assert_eq!(t.charset(), Charset::Ascii, "ASCII by default");
    feed(&mut t, b"\x1b(A");
    assert_eq!(t.charset(), Charset::Uk, "UK is reported as UK");
    feed(&mut t, b"\x1b(B");
    assert_eq!(t.charset(), Charset::Ascii, "and `B` returns to ASCII");
}
