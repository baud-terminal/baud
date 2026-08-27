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
