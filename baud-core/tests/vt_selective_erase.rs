//! DECSCA, DECSED and DECSEL — character protection and selective erase.
//!
//! One feature under three names, which is why they land together: implementing
//! the erases without the protection would make them silent aliases for ED and
//! EL, and an application that protected a region would watch it vanish.
//!
//! * `[[source: vt510-decsca]]`: "Ps defines all characters that follow the
//!   DECSCA function as erasable or not erasable... 1 DECSED and DECSEL cannot
//!   erase characters." And, load-bearing for the implementation: "DECSCA does
//!   not effect visual character attributes set by the select graphic rendition
//!   (SGR) function."
//! * `[[source: vt510-decsed]]`: "erases some or all of the erasable characters
//!   in the display... DECSED works inside or outside the scrolling margins."
//! * `[[source: vt510-decsel]]`: the same, for one line.

use baud_core::*;

fn grid(bytes: &[u8]) -> Vec<PackedCell> {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let mut i = 0;
    while i < bytes.len() {
        let n = READ_CHUNK.min(bytes.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let filled = slot.write(StreamByte::slice_from(&bytes[i..i + n]));
        t.feed(base, filled.len());
        t.commit_read(filled.len());
        i += n;
    }
    t.viewport().to_vec()
}

/// Text written while protected survives DECSEL.
#[test]
fn decsel_spares_protected_characters() {
    // "AB" protected, "CD" not; erase the whole line selectively.
    let after = grid(b"\x1b[1\"qAB\x1b[0\"qCD\x1b[1;1H\x1b[?2K");
    let only_ab = grid(b"\x1b[1\"qAB");
    assert_eq!(after, only_ab, "the protected AB must remain, CD must go");
}

/// And DECSED, over the display.
#[test]
fn decsed_spares_protected_characters() {
    let after = grid(b"\x1b[1\"qAB\r\n\x1b[0\"qCD\x1b[1;1H\x1b[?2J");
    let only_ab = grid(b"\x1b[1\"qAB");
    assert_eq!(after, only_ab);
}

/// Unprotected text is erased exactly as ED/EL would erase it.
#[test]
fn selective_erase_matches_plain_erase_when_nothing_is_protected() {
    assert_eq!(
        grid(b"hello\x1b[1;1H\x1b[?0K"),
        grid(b"hello\x1b[1;1H\x1b[0K")
    );
    assert_eq!(
        grid(b"a\r\nb\r\nc\x1b[1;1H\x1b[?0J"),
        grid(b"a\r\nb\r\nc\x1b[1;1H\x1b[0J")
    );
}

/// A plain ED/EL is **not** selective: it erases protected characters too.
///
/// This is the pair most easily conflated, and conflating them would make the
/// protection meaningless.
#[test]
fn plain_erase_ignores_protection() {
    let selective = grid(b"\x1b[1\"qAB\x1b[1;1H\x1b[?2K");
    let plain = grid(b"\x1b[1\"qAB\x1b[1;1H\x1b[2K");
    assert_ne!(selective, plain, "DECSEL must spare what EL erases");
}

/// `DECSCA 0` and `DECSCA 2` both mean erasable.
///
/// `[[source: vt510-decsca]]`: "2 Same as 0."
#[test]
fn decsca_zero_and_two_both_mean_erasable() {
    let via_zero = grid(b"\x1b[1\"qA\x1b[0\"qB\x1b[1;1H\x1b[?2K");
    let via_two = grid(b"\x1b[1\"qA\x1b[2\"qB\x1b[1;1H\x1b[?2K");
    assert_eq!(via_zero, via_two);
}

/// An omitted parameter is 0, i.e. erasable.
#[test]
fn decsca_defaults_to_erasable() {
    let after = grid(b"\x1b[1\"qA\x1b[\"qB\x1b[1;1H\x1b[?2K");
    let only_a = grid(b"\x1b[1\"qA");
    assert_eq!(
        after, only_a,
        "B was written unprotected and must be erased"
    );
}

/// Protection is not an SGR attribute.
///
/// `[[source: vt510-decsca]]`: "DECSCA does not effect visual character
/// attributes set by the select graphic rendition (SGR) function." Turning
/// protection on must not disturb bold, and turning bold on must not disturb
/// protection.
#[test]
fn protection_and_sgr_are_independent() {
    // Bold survives a DECSCA.
    assert_eq!(
        grid(b"\x1b[1m\x1b[1\"qX\x1b[0\"q"),
        grid(b"\x1b[1\"q\x1b[1mX\x1b[0\"q"),
        "order of SGR and DECSCA must not matter"
    );
    // Protection survives an SGR: the SGR must not reset the protection that
    // DECSCA set, so the two cells are still there afterwards.
    let before = grid(b"\x1b[1\"q\x1b[1mAB");
    let after = grid(b"\x1b[1\"q\x1b[1mAB\x1b[1;1H\x1b[?2K");
    assert_eq!(
        after[..2],
        before[..2],
        "an SGR after DECSCA must not clear the protection"
    );
    // Control: without the DECSCA the same two cells are erased, so the
    // assertion above is not vacuous.
    let unprotected = grid(b"\x1b[1m AB\x1b[1;1H\x1b[?2K");
    assert_ne!(after[..2], unprotected[..2]);
}

/// An erased position is erasable again.
///
/// The blank an erase leaves behind carries the current rendition -- that is
/// erase-to-background -- but it must not carry the protection with it.
/// Otherwise the first DECSEL under `DECSCA 1` protects every cell it just
/// cleared, and a second one finds an unerasable line.
#[test]
fn an_erased_cell_is_not_protected() {
    // Same erase, once with DECSCA in force and once without. Both clear the
    // whole line, so the only thing that could differ is what the blanks carry.
    let under_protection = grid(b"\x1b[1\"qxxxx\x1b[1;1H\x1b[2K");
    let plain = grid(b"\x1b[0\"qxxxx\x1b[1;1H\x1b[2K");
    assert_eq!(
        under_protection, plain,
        "a blank must not inherit the DECSCA protection"
    );
    assert_eq!(under_protection, grid(b""), "and it is an ordinary blank");
}

/// Selective erase works outside the scrolling margins, unlike operations that
/// are confined to the region.
///
/// `[[source: vt510-decsed]]`: "DECSED works inside or outside the scrolling
/// margins."
#[test]
fn selective_erase_is_not_confined_to_the_region() {
    // Write on row 0, set a region well below it, then erase the display from
    // row 0. The text must go even though row 0 is outside the region.
    let after = grid(b"XY\x1b[10;20r\x1b[1;1H\x1b[?2J");
    assert_eq!(after, grid(b""), "row 0 must be erased despite the region");
}

/// RIS clears the protection along with the rest of the rendition, so a program
/// cannot leave the next one with an unerasable screen.
#[test]
fn ris_clears_protection() {
    let after = grid(b"\x1b[1\"q\x1bcAB\x1b[1;1H\x1b[?2K");
    assert_eq!(after, grid(b""), "after RIS, AB is erasable again");
}
