//! DECSET / DECRST: the modes baud actually acts on.
//!
//! DECSET/DECRST were `parsed` — recognised and dropped. Only modes baud acts
//! on get a bit, so mouse reporting, bracketed paste and the alternate screen
//! are deliberately still ignored rather than recorded: a bit that is set and
//! never read is the same "accepted, did nothing" failure.
//!
//! **A source conflict, recorded rather than smoothed over.**
//! `[[source: vt510-decawm]]` says of DECAWM: "Default: No autowrap".
//! `[[source: ctlseqs]]` documents xterm, where autowrap is on by default, and
//! every modern terminal wraps. baud follows xterm: an application that emits a
//! long line expects it to wrap, and defaulting to the VT510 behaviour would
//! break essentially all of them. The DEC default is the older and stricter
//! reading, and it is the one baud does not implement.

use crate::support;
use baud_core::*;

fn term(bytes: &[u8]) -> (usize, usize) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    let (r, c) = t.cursor();
    (r.index(), c.index())
}

fn visible(bytes: &[u8]) -> bool {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index((bytes).len()));
    t.cursor_visible()
}

// ── DECTCEM ─────────────────────────────────────────────────────────────────

/// The cursor is visible on a fresh terminal, and `?25` toggles it.
///
/// baud has no renderer, but an embedder needs the flag; dropping it would
/// leave one no way to honour `?25 l`.
#[test]
fn dectcem_toggles_and_defaults_visible() {
    assert!(visible(b""), "a fresh terminal shows its cursor");
    assert!(!visible(b"\x1b[?25l"));
    assert!(visible(b"\x1b[?25l\x1b[?25h"));
}

// ── DECAWM ──────────────────────────────────────────────────────────────────

/// Autowrap is on by default: text past the right margin continues on the next
/// line. Following xterm, not the VT510 manual — see the module note.
#[test]
fn autowrap_is_on_by_default() {
    // 200 columns; 205 characters must land on row 1.
    let long: Vec<u8> = std::iter::repeat(b'x').take(205).collect();
    assert_eq!(term(&long), (1, 5));
}

/// With DECAWM reset the cursor sticks at the last column and characters
/// overwrite it. No wrap means no scroll.
#[test]
fn decawm_reset_stops_at_the_right_margin() {
    let mut v = b"\x1b[?7l".to_vec();
    v.extend(std::iter::repeat(b'x').take(205));
    assert_eq!(term(&v), (0, 199), "must stay on row 0, at the last column");
}

/// Re-setting it restores wrapping.
#[test]
fn decawm_can_be_set_again() {
    let mut v = b"\x1b[?7l\x1b[?7h".to_vec();
    v.extend(std::iter::repeat(b'x').take(205));
    assert_eq!(term(&v), (1, 5));
}

/// A no-wrap line must not scroll the screen either — the danger is that
/// "stop at the margin" is implemented as "wrap without moving", which still
/// scrolls at the bottom row.
#[test]
fn decawm_reset_does_not_scroll_at_the_last_row() {
    let mut v = b"\x1b[?7l\x1b[50;1H".to_vec();
    v.extend(std::iter::repeat(b'x').take(400));
    assert_eq!(term(&v), (49, 199), "cursor pinned to the last cell");
}

// ── DECOM ───────────────────────────────────────────────────────────────────

/// With DECOM set, row addressing is relative to the top margin.
///
/// `[[source: vt510-decom]]`: "The starting point for line numbers depends on
/// the current top margin setting."
#[test]
fn decom_makes_row_addressing_relative_to_the_margin() {
    // Region rows 4..10 (1-based). `CSI 1 ; 1 H` under DECOM is the region top.
    assert_eq!(term(b"\x1b[5;10r\x1b[?6h\x1b[1;1H"), (4, 0));
    assert_eq!(
        term(b"\x1b[5;10r\x1b[?6h\x1b[3;1H"),
        (6, 0),
        "third line of the region"
    );
    // Without DECOM the same CUP is absolute.
    assert_eq!(term(b"\x1b[5;10r\x1b[?6l\x1b[1;1H"), (0, 0));
}

/// VPA is relative too, since it is row addressing by another name.
#[test]
fn decom_applies_to_vpa() {
    assert_eq!(term(b"\x1b[5;10r\x1b[?6h\x1b[2d"), (5, 0));
}

/// The cursor cannot move outside the margins while DECOM is set.
///
/// `[[source: vt510-decom]]`: "The cursor cannot move outside of the margins."
#[test]
fn decom_confines_the_cursor_to_the_margins() {
    // Above the region.
    assert_eq!(term(b"\x1b[5;10r\x1b[?6h\x1b[1;1H\x1b[9A").0, 4);
    // Below it: an absolute address past the bottom clamps to the bottom.
    assert_eq!(term(b"\x1b[5;10r\x1b[?6h\x1b[40;1H").0, 9);
}

/// Setting or resetting DECOM homes the cursor, and home depends on the mode
/// just applied.
#[test]
fn decom_homes_the_cursor_on_change() {
    assert_eq!(
        term(b"\x1b[5;10r\x1b[20;5H\x1b[?6h"),
        (4, 0),
        "home is the margin"
    );
    assert_eq!(
        term(b"\x1b[5;10r\x1b[20;5H\x1b[?6l"),
        (0, 0),
        "home is the screen"
    );
}

/// A mode baud does not implement is ignored, not recorded and not acted on.
#[test]
fn unimplemented_private_modes_are_inert() {
    // Bracketed paste, alt screen, mouse: parsed, deliberately no effect here.
    assert_eq!(term(b"\x1b[?2004h\x1b[?1049h\x1b[?1000habc"), (0, 3));
}

/// **Eviction makes a cell report a colour it was never written with.**
///
/// `PackedCell` holds a `StyleId` index into a direct-mapped table that evicts
/// on collision, and `Term::style_of` reads the slot back to answer what a cell
/// looks like. When a later style lands in the same slot, every earlier cell
/// pointing at it starts reporting the newcomer's rendition -- not a blank, not
/// a default, but a different real colour.
///
/// btop draws in truecolour and interns 109,274 styles against 799,360 SGR
/// dispatches at `NSTYLE = 64`: a 13.7% miss rate, and every miss is an
/// eviction that silently repaints history.
#[test]
fn a_full_style_table_makes_cells_report_the_wrong_colour() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    // One distinct truecolour foreground per column, then read them all back.
    let n = 200usize;
    let mut b = Vec::new();
    b.extend_from_slice(b"\x1b[1;1H");
    for i in 0..n {
        let (r, g, bl) = (i % 256, (i * 7) % 256, (i * 13) % 256);
        b.extend_from_slice(format!("\x1b[38;2;{r};{g};{bl}mX").as_bytes());
    }
    let mut i = 0;
    while i < b.len() {
        let k = READ_CHUNK.min(b.len() - i);
        t.write_at(BytePos::ZERO, &b[i..i + k]);
        t.feed(BytePos::ZERO, ReadLen::from_index((b[i..i + k]).len()));
        i += k;
    }

    let row = t.row(Row::from_index(0));
    let mut wrong = 0usize;
    for i in 0..n {
        let (r, g, bl) = (i % 256, (i * 7) % 256, (i * 13) % 256);
        // `Chan::from_offset(v, 0)` is the public way in; `Param` is private.
        let chan = |v: usize| Chan::from_offset(v as u16, 0);
        let want = ColorWord::rgb(chan(r), chan(g), chan(bl));
        if t.style_of(row[i]).fg() != want {
            wrong += 1;
        }
    }
    assert_eq!(
        wrong, 0,
        "{wrong} of {n} cells report a foreground they were never written \
         with, because the style table evicted their slot"
    );
}

/// **The palette a real program uses must not collide in the style table.**
///
/// The table is direct-mapped with eviction, so two live renditions sharing a
/// bucket evict each other on every alternation -- and a cell holds a
/// `StyleId`, which is the bucket, so the cells written under the first one
/// silently repaint in the second. Nothing reports an error; the terminal just
/// shows the wrong colour.
///
/// This asserts the property directly: the thirty-two renditions of the
/// sixteen-colour palette, normal and bold, land in thirty-two distinct slots.
/// A 1024-entry table has room for them thirty-two times over, so a collision
/// here is a hash fault and not a capacity one.
///
/// It is the case `a_full_style_table_makes_cells_report_the_wrong_colour`
/// cannot reach: that one over-subscribes the table on purpose with 200
/// truecolours, where eviction is the *correct* behaviour. Here there is no
/// excuse for it.
///
/// **`asciiquarium.raw` is why this exists.** It emits thirteen distinct SGR
/// strings and was interning **14,769** times across two passes -- continuous
/// eviction on real capture -- because the key XORed `fg`, `bg << 21` and
/// `attrs << 45`, whose bit ranges overlap, and ignored `underline` and `font`
/// entirely.
#[test]
fn the_sixteen_colour_palette_does_not_collide() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let mut b = Vec::new();
    let mut want = Vec::new();
    let mut col = 0usize;
    for bold in [0usize, 1] {
        for base in [30usize, 90] {
            for c in 0..8usize {
                b.extend_from_slice(
                    format!("\x1b[1;{}H\x1b[0;{bold};{}mX", col + 1, base + c).as_bytes(),
                );
                want.push(col);
                col += 1;
            }
        }
    }
    let mut i = 0;
    while i < b.len() {
        let k = READ_CHUNK.min(b.len() - i);
        t.write_at(BytePos::ZERO, &b[i..i + k]);
        t.feed(BytePos::ZERO, ReadLen::from_index((b[i..i + k]).len()));
        i += k;
    }

    let row = t.row(Row::from_index(0));
    let mut slots = Vec::new();
    for c in &want {
        slots.push(row[*c].style.index());
    }
    let mut sorted = slots.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        slots.len(),
        "{} of the {} palette renditions share a style slot with another. The \
         table is direct-mapped and evicts, so those cells repaint in each \
         other's colour whenever both are on screen",
        slots.len() - sorted.len(),
        slots.len()
    );
}
/// **Renditions that differ must still differ after they are written.**
///
/// The style table is direct-mapped and evicts, and a cell holds a `StyleId`
/// which *is* the bucket. So when two live renditions share a bucket, the
/// second to arrive takes the slot and every cell already pointing at it
/// silently repaints in the new colour.
///
/// **That failure is invisible to the obvious test.** Reading the cells back
/// afterwards shows both of them holding the *same* rendition -- the evicted
/// one is simply gone -- so a check for "two cells with one slot but different
/// colours" finds nothing. What it costs is *distinctness*: 28 renditions go
/// in and fewer than 28 come out.
///
/// Counting them is therefore the test. These are the renditions
/// `asciiquarium.raw` actually produces -- its thirteen SGR strings in every
/// pairing, which is how `40` (background black, 16,632 of them) combines with
/// each foreground -- and there are 28 distinct ones. The old key XORed `fg`,
/// `bg << 21` and `attrs << 45`, whose ranges overlap, and collapsed them to
/// **18**: ten renditions on screen at once wearing each other's colours,
/// continuously, on a recording in this repo.
#[test]
fn renditions_that_differ_still_differ_after_a_write() {
    let sgrs = [
        "40", "37", "0;1", "0", "36", "34", "33", "32", "31", "30", "35", "39;49", "",
    ];
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let mut b = Vec::new();
    let mut n = 0usize;
    for a in &sgrs {
        for c in &sgrs {
            b.extend_from_slice(format!("\x1b[1;{}H\x1b[0m\x1b[{a}m\x1b[{c}mX", n + 1).as_bytes());
            n += 1;
        }
    }
    let mut i = 0;
    while i < b.len() {
        let k = READ_CHUNK.min(b.len() - i);
        t.write_at(BytePos::ZERO, &b[i..i + k]);
        t.feed(BytePos::ZERO, ReadLen::from_index((b[i..i + k]).len()));
        i += k;
    }

    let row = t.row(Row::from_index(0));
    let mut seen: Vec<(ColorWord, ColorWord, bool)> = Vec::new();
    for c in 0..n {
        let w = t.style_of(row[c]);
        let k = (w.fg(), w.bg(), w.attrs().has(Attrs::BOLD));
        if !seen.contains(&k) {
            seen.push(k);
        }
    }
    assert_eq!(
        seen.len(),
        28,
        "{} distinct renditions survived out of 28. A 1024-entry table has room \
         for all of them, so a shortfall is two of them sharing a bucket and \
         evicting each other -- and the cells that lost the race are now \
         showing the wrong colour",
        seen.len()
    );
}
