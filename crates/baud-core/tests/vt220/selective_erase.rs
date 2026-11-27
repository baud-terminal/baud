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

use crate::support;
use baud_core::*;

fn grid(bytes: &[u8]) -> Vec<PackedCell> {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    support::screen(&t)
}

/// Text written while protected survives DECSEL; the erasable `CD` become
/// spaces (`[[source: dec-std-070]]` p. 5-159, below).
#[test]
fn decsel_spares_protected_characters() {
    // "AB" protected, "CD" not; erase the whole line selectively.
    let after = grid(b"\x1b[1\"qAB\x1b[0\"qCD\x1b[1;1H\x1b[?2K");
    let ab_then_spaces = grid(b"\x1b[1\"qAB\x1b[0\"q  ");
    assert_eq!(
        after, ab_then_spaces,
        "the protected AB must remain, CD must go"
    );
}

/// And DECSED, over the display.
#[test]
fn decsed_spares_protected_characters() {
    let after = grid(b"\x1b[1\"qAB\r\n\x1b[0\"qCD\x1b[1;1H\x1b[?2J");
    let ab_then_spaces = grid(b"\x1b[1\"qAB\r\n\x1b[0\"q  ");
    assert_eq!(after, ab_then_spaces);
}

/// With nothing protected a selective erase still differs from ED and EL:
/// it writes a space where they leave an empty position. Until 2026-10-19
/// the two agreed, which is xterm's `ClearCells` and not STD 070's
/// DECSEL and DECSED, "changed to the Space character (2/0)".
#[test]
fn selective_erase_writes_spaces_where_plain_erase_empties() {
    assert_eq!(grid(b"hello\x1b[1;1H\x1b[?0K"), grid(b"     "));
    assert_eq!(grid(b"hello\x1b[1;1H\x1b[0K"), grid(b""));
    assert_eq!(grid(b"a\r\nb\r\nc\x1b[1;1H\x1b[?0J"), grid(b" \r\n \r\n "));
    assert_eq!(grid(b"a\r\nb\r\nc\x1b[1;1H\x1b[0J"), grid(b""));
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
    let a_then_space = grid(b"\x1b[1\"qA\x1b[0\"q ");
    assert_eq!(
        after, a_then_space,
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
    assert_eq!(
        after,
        grid(b"  "),
        "row 0 must be erased despite the region"
    );
}

/// Protection survives style-table eviction.
///
/// The style table evicts once it is full: past capacity, interning a new
/// rendition overwrites whichever one shared its bucket. Protection therefore
/// cannot be stored there and looked back up, which is what an earlier version
/// did -- it answered with whatever style had displaced the original, so
/// protection was silently lost (or granted to text that never asked for it)
/// as soon as an application cycled through more distinct renditions than the
/// table holds. The table is sized to the screen now, so a still image no
/// longer overflows it, but an application cycling colours over time still
/// does, and protection must survive that.
///
/// Deterministic: the cycle is forced, not searched for.
#[test]
fn protection_survives_style_eviction() {
    // The table is sized to the screen now, so "more than the table" is
    // `style_capacity()`, not a fixed 1024 -- mint half again past it so every
    // bucket is overwritten and the table genuinely turns over.
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let cap = t.styles().one().len();
    support::feed(&mut t, b"\x1b[1\"qAB\x1b[0\"q");
    for i in 0..(cap + cap / 2) {
        // A distinct 24-bit colour per `i`, so the count of distinct renditions
        // is the iteration count and it provably exceeds `cap`.
        let (r, g, b) = ((i >> 16) & 0xff, (i >> 8) & 0xff, i & 0xff);
        support::feed(&mut t, format!("\x1b[38;2;{r};{g};{b}m").as_bytes());
    }
    support::feed(&mut t, b"\x1b[1;1H\x1b[?2K");
    let after = support::screen(&t);
    let only_ab = grid(b"\x1b[1\"qAB");
    assert_eq!(
        after[..2],
        only_ab[..2],
        "AB must still be protected after the table has turned over"
    );
}

/// Exactly the protected cells survive -- not "each cell took one of two
/// allowed values", which both false-protection and false-erasure satisfy.
#[test]
fn selective_erase_spares_exactly_the_protected_cells() {
    // Alternate protected and unprotected characters across ten columns.
    let mut v = Vec::new();
    for i in 0..10 {
        v.extend_from_slice(if i % 2 == 0 {
            b"\x1b[1\"q"
        } else {
            b"\x1b[0\"q"
        });
        v.push(b'a' + i);
    }
    let before = grid(&v);
    v.extend_from_slice(b"\x1b[1;1H\x1b[?2K");
    let after = grid(&v);
    let space = grid(b" ")[0];
    for i in 0..10usize {
        if i % 2 == 0 {
            assert_eq!(
                after[i], before[i],
                "column {i} was protected and must remain"
            );
        } else {
            assert_eq!(
                after[i], space,
                "column {i} was erasable and must be a space"
            );
        }
    }
}

/// DECSTR returns the protection to normal.
///
/// `[[source: vt510-decstr]]`, Table 5-9: "Select character attribute / DECSCA
/// / Normal (erasable by DECSEL and DECSED)."
#[test]
fn decstr_clears_protection() {
    let after = grid(b"\x1b[1\"q\x1b[!pAB\x1b[1;1H\x1b[?2K");
    assert_eq!(after, grid(b"  "), "after DECSTR, AB is erasable again");
}

/// DECSC saves the protection and DECRC restores it.
///
/// `[[source: vt510-decsc]]` lists "Selective erase attribute" among what the
/// save keeps, so a program can set protection, save, change it, restore, and
/// find the original in force.
#[test]
fn decsc_and_decrc_carry_the_protection() {
    // Protect, save, unprotect, restore, then write: the write must be
    // protected because the restore brought DECSCA 1 back.
    let after = grid(b"\x1b[1\"q\x1b7\x1b[0\"q\x1b8AB\x1b[1;1H\x1b[?2K");
    let only_ab = grid(b"\x1b[1\"qAB");
    assert_eq!(
        after[..2],
        only_ab[..2],
        "DECRC must restore the protection"
    );
    // And the converse: saving while unprotected restores unprotected.
    let gone = grid(b"\x1b[0\"q\x1b7\x1b[1\"q\x1b8AB\x1b[1;1H\x1b[?2K");
    assert_eq!(gone, grid(b"  "), "DECRC must restore the *saved* state");
}

/// RIS clears the protection along with the rest of the rendition, so a program
/// cannot leave the next one with an unerasable screen.
#[test]
fn ris_clears_protection() {
    let after = grid(b"\x1b[1\"q\x1bcAB\x1b[1;1H\x1b[?2K");
    assert_eq!(after, grid(b"  "), "after RIS, AB is erasable again");
}

// ── DEC STD 070: an erased character becomes a space ────────────────────────
//
// `[[source: dec-std-070]]` DECSEL (p. 5-159) and DECSED (p. 5-162): the
// erasable positions are "changed to the Space character (2/0). The
// character rendition and attributes associated with each position do not
// change. Characters which have been entered into the display with the
// Selectively Erasable attribute reset (Not Selectively Erasable), as well
// as empty character positions, are not affected by this control". Their
// algorithms assign `display[line][x].code = 32` and nothing else (pp. 5-160,
// 5-163). Until 2026-10-19 baud wrote an empty cell in the current rendition,
// empty positions included, as xterm's `ClearCells` does.

const SMALL: WinSize = WinSize {
    ws_row: 4,
    ws_col: 20,
};

/// A cell as it reads: its content, its rendition resolved (a `StyleId`
/// names a slot, which differs between terminals) and its flags.
type Seen = (Content, StyleWord, baud_domain::CellFlags);

/// The first `n` cells of row `y` after `bytes`.
fn cells(bytes: &[u8], y: usize, n: usize) -> Vec<Seen> {
    support::render(&SMALL, bytes, |t| {
        t.row(Row::from_index(y)).expect("a row")[..n]
            .iter()
            .map(|c| (c.content, t.styles().one().word_of(*c), c.flags))
            .collect()
    })
}

/// The cells `bytes` writes from home, the expected result spelled as text.
fn written(bytes: &[u8], n: usize) -> Vec<Seen> {
    cells(bytes, 0, n)
}

fn is_space(c: &Seen) -> bool {
    c.0.as_codepoint().map(|c| c.value()) == Some(0x20)
}

/// DECSEL writes a space in the rendition the character had, not the
/// current one.
#[test]
fn decsel_turns_a_character_into_a_space_in_its_own_rendition() {
    let after = cells(b"\x1b[31mAB\x1b[0m\x1b[1;1H\x1b[?2K", 0, 3);
    let want = written(b"\x1b[31m  \x1b[0m", 3);
    assert_eq!(after, want);
    assert!(is_space(&after[0]));
}

/// And DECSED, over every row it covers.
#[test]
fn decsed_turns_characters_into_spaces_in_their_own_rendition() {
    let bytes = b"\x1b[32ma\r\n\x1b[44mb\x1b[0m\x1b[1;1H\x1b[?2J";
    assert_eq!(cells(bytes, 0, 2), written(b"\x1b[32m \x1b[0m", 2));
    assert_eq!(cells(bytes, 1, 2), written(b"\x1b[32;44m \x1b[0m", 2));
}

/// An empty position is not affected: it stays empty, in the rendition an
/// earlier erase gave it, under DECSEL and DECSED alike.
#[test]
fn an_empty_position_is_not_affected() {
    let erased_red = cells(b"\x1b[41m\x1b[2K\x1b[0m", 0, 4);
    assert!(erased_red[0].0.is_empty(), "the control: an empty cell");
    assert_eq!(cells(b"\x1b[41m\x1b[2K\x1b[0m\x1b[?2K", 0, 4), erased_red);
    assert_eq!(cells(b"\x1b[41m\x1b[2K\x1b[0m\x1b[?2J", 0, 4), erased_red);
}

/// A wide character the span's edge cuts goes whole, both halves a space in
/// its rendition, since 2026-10-19. DECSEL 0 from the tail and DECSEL 1 to
/// the head.
#[test]
fn a_wide_character_the_edge_cuts_becomes_two_spaces() {
    let want = written(b"\x1b[35m  \x1b[0mz", 3);
    assert_eq!(
        cells("\x1b[35m\u{4e16}\x1b[0mz\x1b[1;2H\x1b[?0K".as_bytes(), 0, 3)[..2],
        want[..2]
    );
    assert_eq!(
        cells("\x1b[35m\u{4e16}\x1b[0mz\x1b[1;1H\x1b[?1K".as_bytes(), 0, 3),
        want
    );
    assert_eq!(
        cells("\x1b[35m\u{4e16}\x1b[0mz\x1b[1;2H\x1b[?0J".as_bytes(), 0, 2),
        want[..2]
    );
}

/// A protected wide character the edge cuts stays whole.
#[test]
fn a_protected_wide_character_the_edge_cuts_stays() {
    let before = cells("\x1b[1\"q\u{4e16}\x1b[0\"q".as_bytes(), 0, 2);
    assert_eq!(
        cells(
            "\x1b[1\"q\u{4e16}\x1b[0\"q\x1b[1;2H\x1b[?0K".as_bytes(),
            0,
            2
        ),
        before
    );
    assert_eq!(
        cells(
            "\x1b[1\"q\u{4e16}\x1b[0\"q\x1b[1;1H\x1b[?1K".as_bytes(),
            0,
            2
        ),
        before
    );
}

/// The guard SPA and EPA set stays (since 2026-10-16) and so does an
/// `OSC 8` link; the line-drawing set's flag and a cluster's marks go
/// with the character.
#[test]
fn the_space_keeps_the_guard_and_the_link_and_drops_the_rest() {
    let g = cells(b"\x1bVab\x1bW\x1b[1;1H\x1b[?2K", 0, 2);
    assert!(g.iter().all(|c| c.2.is_guarded()), "the guard stays");
    assert!(g.iter().all(is_space));
    support::render(
        &SMALL,
        b"\x1b]8;;http://x/\x1b\\ab\x1b]8;;\x1b\\\x1b[1;1H\x1b[?2K",
        |t| {
            let c = t.row(Row::ZERO).expect("a row")[0];
            assert_eq!(c.content.as_codepoint().map(|c| c.value()), Some(0x20));
            assert_eq!(t.link_uri(c.flags.link()), Some(&b"http://x/"[..]));
        },
    );
    let d = cells(b"\x1b(0q\x1b(B\x1b[1;1H\x1b[?2K", 0, 1);
    assert_eq!(d, written(b" ", 1), "no line-drawing flag on the space");
    support::render(&SMALL, "e\u{301}\x1b[1;1H\x1b[?2K".as_bytes(), |t| {
        let c = t.row(Row::ZERO).expect("a row")[0];
        assert_eq!(c.content.as_codepoint().map(|c| c.value()), Some(0x20));
        assert_eq!(t.cluster_marks(c).count(), 0, "the mark went");
        assert!(!c.flags.is_marked(), "MARKED went");
    });
}

/// A row DECALN filled, whose cells past its mark are all `E`, is spaces
/// to its end after DECSED 2.
#[test]
fn decsed_reaches_past_the_rows_mark() {
    for y in 0..4 {
        assert_eq!(
            cells(b"\x1b#8\x1b[?2J", y, 20),
            written(&[b' '; 20], 20),
            "row {y}"
        );
    }
}

/// **DECSEL and DECSED apply every parameter in order**
/// (`[[source: dec-std-070]]` pp. 5-160 and 5-163, `for (n=0; n<parc;
/// n++)`): `CSI ? 1 ; 0 K` turns the erasable characters either side of
/// the cursor into spaces and spares the protected one under it.
#[test]
fn the_selective_erases_apply_every_parameter_in_order() {
    let after = grid(b"ab\x1b[1\"qP\x1b[0\"qcd\x1b[1;3H\x1b[?1;0K");
    let want = grid(b"  \x1b[1\"qP\x1b[0\"q  ");
    assert_eq!(after, want);
    let after = grid(b"ab\r\ncd\r\nef\x1b[2;2H\x1b[?0;1J");
    let want = grid(b"  \r\n  \r\n  ");
    assert_eq!(after, want);
    // The control: the first alone leaves the right side.
    assert_ne!(grid(b"abcd\x1b[1;2H\x1b[?1K"), grid(b"    "));
}
