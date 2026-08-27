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

/// **A byte under the line-drawing set is translated, and stays marked.**
///
/// This asserted the byte was *untouched*, "the mapping needs a source baud
/// lacks". The DEC manual's table really is pictures -- `MANIFEST.toml` records
/// that the page was fetched and checked -- but `[[source: foot-vt100-0]]` is
/// the same mapping in machine-readable form, cited as foot's rather than as
/// DEC's, because that is whose judgement it is.
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

/// Bytes outside `` ` ``..`~` are ASCII even under the graphics set.
#[test]
fn graphics_leaves_the_rest_of_ascii_alone() {
    let (g, _) = run(b"\x1b(0A");
    assert_eq!(
        g[0].content,
        run(b"A").0[0].content,
        "the set differs from ASCII only in 0x60..0x7e"
    );
    assert!(g[0].flags.is_graphics(), "still marked, though");
}

/// The table matches the pinned source.
#[test]
fn dec_graphics_table_matches_the_pinned_source() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let out = std::process::Command::new("python3")
        .arg(format!("{root}/scripts/gen-dec-graphics.py"))
        .arg("--check")
        .current_dir(root)
        .output()
        .expect("python3 to run the generator");
    assert!(
        out.status.success(),
        "dec_graphics.rs is stale -- rerun scripts/gen-dec-graphics.py\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
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

/// **Every single-byte charset final `[[source: ctlseqs]]` lists is named.**
///
/// They all collapsed into `Charset::Other`, which meant an embedder could not
/// tell Finnish from a byte baud did not recognise — and "the term *replacement*
/// says that the character set is formed by replacing some of the characters
/// with more useful ones for a given language", so *which* one it is decides
/// which characters change.
///
/// Recorded, not translated: the replacements are a glyph mapping and
/// `MANIFEST.toml` carries no machine-readable DEC source for one. The
/// distinction this pins is *named but untranslated* versus *indistinguishable
/// from garbage*.
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
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let mut slot = t.read_slot();
    let base = slot.base();
    let f = slot.write(StreamByte::slice_from(b"\x1b(%z"));
    t.feed(base, f.len());
    t.commit_read(f.len());
    assert_eq!(t.unsupported.count(), 1, "counted");
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

/// **The line-drawing table covers the whole set, not part of it.**
///
/// DEC Special Graphics differs from ASCII only in `0x60..0x7E`, so 31 entries
/// is the complete mapping rather than a useful subset. `MANIFEST.toml` still
/// records `dec-special-graphics` as `gated` — the DEC-authored table was
/// never obtained in machine-readable form and that stays true — while
/// `[[source: foot-vt100-0]]` supplies the mapping. Reading the gate as a
/// missing capability is the confusion this pins.
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
    // And nothing outside the range is touched.
    for byte in [b'A', b'Z', b'0', b'9', b' ', b'_'] {
        let mut bytes = b"\x1b(0".to_vec();
        bytes.push(byte);
        let (g, _) = run(&bytes);
        assert_eq!(
            g[0].content.as_codepoint().map(|c| c.value()),
            Some(u32::from(byte)),
            "0x{byte:02x} is outside 0x60..0x7e and is ASCII"
        );
    }
}
