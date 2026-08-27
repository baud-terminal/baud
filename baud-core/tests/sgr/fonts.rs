//! `SGR 10-19` — font selection.
//!
//! `docs/vt-coverage.md` listed these **out**, with the reason "no renderer
//! consumes them, and a bit set and never read is the failure this table exists
//! to catch". The second half is a real rule and the first half was an
//! assumption about where a font lives.
//!
//! A font is a *rendition*, not a property of a cell's content, and rendition
//! lives in `StyleWord` — which carried sixteen bits of explicit padding, put
//! there because zerocopy rejects implicit padding. `PackedCell` never had to
//! grow, and the eight-byte cell was never the constraint.
//!
//! And "never read" resolves the way it did for charset designations and
//! `?1001`: baud records the selection and a renderer reads it off the cell's
//! style. Choosing a face needs font machinery a `no_std` core with no
//! allocator does not have; recording which face was asked for does not.

use baud_core::*;

fn style_at(bytes: &[u8], col: usize) -> StyleWord {
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
    let cell = t.viewport()[col];
    t.style_of(cell)
}

fn replies(bytes: &[u8]) -> Vec<u8> {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let mut slot = t.read_slot();
    let base = slot.base();
    let f = slot.write(StreamByte::slice_from(bytes));
    t.feed(base, f.len());
    t.commit_read(f.len());
    t.reply().to_vec()
}

/// Every alternate font is recorded.
#[test]
fn sgr_11_to_19_select_the_alternates() {
    for n in 11..=19u16 {
        let mut bytes = b"\x1b[".to_vec();
        bytes.extend_from_slice(n.to_string().as_bytes());
        bytes.extend_from_slice(b"mX");
        assert_eq!(
            style_at(&bytes, 0).font().index(),
            n - 10,
            "SGR {n} is alternate font {}",
            n - 10
        );
    }
}

/// `SGR 10` is the primary font, and is the default.
#[test]
fn sgr_10_is_the_primary_font() {
    assert!(style_at(b"X", 0).font().is_primary(), "default");
    assert!(
        style_at(b"\x1b[13m\x1b[10mX", 0).font().is_primary(),
        "SGR 10 returns to it"
    );
}

/// `SGR 0` resets the font with everything else.
///
/// `[[source: ecma48]]` § 8.3.117: SGR 0 "cancels the effect of any preceding
/// occurrence of SGR in the data stream", which includes the font.
#[test]
fn sgr_0_resets_the_font() {
    assert!(
        style_at(b"\x1b[13m\x1b[0mX", 0).font().is_primary(),
        "reset returns to the primary font"
    );
}

/// The font is part of the rendition, so it separates styles.
///
/// Two cells differing only in font are two styles, not one — which is what
/// makes the interning key correct rather than merely small.
#[test]
fn the_font_is_part_of_the_interning_key() {
    let a = style_at(b"\x1b[31;12mX", 0);
    let b = style_at(b"\x1b[31;13mX", 0);
    assert_ne!(a, b, "same colour, different font, different style");
    assert_eq!(a.fg(), b.fg(), "and the colour really is the same");
}

/// DECRQSS reports the font, and omits it when it is the default.
#[test]
fn decrqss_reports_a_non_default_font() {
    let r = replies(b"\x1b[14m\x1bP$qm\x1b\\");
    assert!(
        r.windows(2).any(|w| w == b"14"),
        "DECRQSS names the font, got {}",
        String::from_utf8_lossy(&r)
    );
    let d = replies(b"\x1bP$qm\x1b\\");
    assert!(
        !d.windows(2).any(|w| w == b"10"),
        "and omits the primary font, got {}",
        String::from_utf8_lossy(&d)
    );
}

// ── SGR 58/59: the underline's own colour ───────────────────────────────────

/// **`SGR 58` sets the underline colour, in both spellings.**
///
/// Listed **out** with the reason that it "needs a third `ColorWord`, which the
/// eight-byte rendition has no room for". Two mistakes in one sentence:
/// `StyleWord` is *twelve* bytes, and `PackedCell` — the eight-byte one — holds
/// a `StyleId` index rather than a rendition. Growing the rendition to sixteen
/// costs `NSTYLE * 4` bytes, which is 256 in a 24 MB `Storage`, and costs the
/// cell nothing. The cell was never being asked.
#[test]
fn sgr_58_sets_the_underline_colour() {
    // Indexed, and the two direct-colour spellings.
    assert_eq!(
        style_at(b"\x1b[58;5;9mX", 0).underline(),
        style_at(b"\x1b[38;5;9mX", 0).fg(),
        "58;5;n names the same colour 38;5;n does"
    );
    assert_eq!(
        style_at(b"\x1b[58;2;10;20;30mX", 0).underline(),
        style_at(b"\x1b[38;2;10;20;30mX", 0).fg(),
        "the legacy five-parameter direct form"
    );
    assert_eq!(
        style_at(b"\x1b[58:2::10:20:30mX", 0).underline(),
        style_at(b"\x1b[38;2;10;20;30mX", 0).fg(),
        "and the ISO 8613-6 colon form with its ignored colour-space id"
    );
}

/// `SGR 59` returns it to the default, which means "follow the foreground".
#[test]
fn sgr_59_resets_the_underline_colour() {
    assert!(
        style_at(b"\x1b[58;5;9m\x1b[59mX", 0)
            .underline()
            .is_default(),
        "59 restores the default"
    );
    assert!(
        style_at(b"\x1b[58;5;9m\x1b[0mX", 0)
            .underline()
            .is_default(),
        "and so does a full reset"
    );
}

/// It is a rendition, so it separates styles and does not disturb the others.
#[test]
fn the_underline_colour_is_independent_of_the_foreground() {
    let a = style_at(b"\x1b[31;58;5;9mX", 0);
    assert_eq!(a.fg(), style_at(b"\x1b[31mX", 0).fg(), "fg is untouched");
    assert_ne!(a.underline(), a.fg(), "and the underline is its own colour");
    assert_ne!(a, style_at(b"\x1b[31mX", 0), "two renditions, not one");
}

/// DECRQSS reports it, and omits it when it is the default.
#[test]
fn decrqss_reports_the_underline_colour() {
    let r = replies(b"\x1b[58;5;9m\x1bP$qm\x1b\\");
    assert!(
        r.windows(2).any(|w| w == b"58"),
        "DECRQSS names it, got {}",
        String::from_utf8_lossy(&r)
    );
    let d = replies(b"\x1bP$qm\x1b\\");
    assert!(
        !d.windows(2).any(|w| w == b"58"),
        "and omits the default, got {}",
        String::from_utf8_lossy(&d)
    );
}

// ── SGR 20, 26/50, 51-55: the presentation attributes ───────────────────────

/// **Fraktur, proportional spacing, framing and overlining are recorded.**
///
/// Out with the reason "no renderer consumes them" — the same reason that kept
/// fonts out, and it resolves the same way: they are renditions, `Attrs` had
/// eight free bits, and `Term::style_of` is where a renderer reads one.
#[test]
fn the_presentation_attributes_are_recorded() {
    let has = |bytes: &[u8], a: Attrs| style_at(bytes, 0).attrs().has(a);
    assert!(has(b"\x1b[20mX", Attrs::FRAKTUR), "20 fraktur");
    assert!(has(b"\x1b[26mX", Attrs::PROPORTIONAL), "26 proportional");
    assert!(has(b"\x1b[51mX", Attrs::FRAMED), "51 framed");
    assert!(has(b"\x1b[52mX", Attrs::ENCIRCLED), "52 encircled");
    assert!(has(b"\x1b[53mX", Attrs::OVERLINED), "53 overlined");
}

/// **`SGR 23` clears two attributes, and `SGR 54` clears two.**
///
/// `[[source: ecma48]]` § 8.3.117 names 23 "not italicized, not fraktur", and
/// 54 clears framed and encircled together. Clearing only one of each pair
/// would leave the other set with no parameter that turns it off.
#[test]
fn the_shared_reset_parameters_clear_both_attributes() {
    let a = style_at(b"\x1b[3;20m\x1b[23mX", 0).attrs();
    assert!(!a.has(Attrs::ITALIC), "23 clears italic");
    assert!(!a.has(Attrs::FRAKTUR), "and fraktur");

    let b = style_at(b"\x1b[51;52m\x1b[54mX", 0).attrs();
    assert!(!b.has(Attrs::FRAMED), "54 clears framed");
    assert!(!b.has(Attrs::ENCIRCLED), "and encircled");

    // 55 clears only overlining, and 50 only proportional spacing.
    assert!(
        !style_at(b"\x1b[53m\x1b[55mX", 0)
            .attrs()
            .has(Attrs::OVERLINED),
        "55 clears overlined"
    );
    assert!(
        !style_at(b"\x1b[26m\x1b[50mX", 0)
            .attrs()
            .has(Attrs::PROPORTIONAL),
        "50 clears proportional"
    );
}

/// DECRQSS walks every attribute, including the new ones.
///
/// `Attrs::WIRE` is a table rather than a `match` precisely so that adding an
/// attribute cannot forget to report it — which is how `fold` once came to
/// discard everything except reset and foreground.
#[test]
fn decrqss_reports_the_presentation_attributes() {
    let r = replies(b"\x1b[20;51;53m\x1bP$qm\x1b\\");
    let s = String::from_utf8_lossy(&r).to_string();
    for want in ["20", "51", "53"] {
        assert!(s.contains(want), "DECRQSS should name {want}, got {s}");
    }
}
