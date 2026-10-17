//! A selection's drawing on the CPU path, where the pixel oracle cannot
//! reach: the oracle compares the instance arm against the cell shader, so
//! a rule both of them lost would pass it. This holds the instance and CPU
//! arms to the rule on their own. `docs/research/notes/k-selection.md` is
//! the spec.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Codepoint, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Damage, Floor, Highlight, PxSize, Region, Renderer, Stack, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_col: 12,
    ws_row: 4,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// U+231A WATCH: East Asian Width `W` with emoji presentation, so the core
/// lays it out two columns wide, and one of the few codepoints the Noto
/// subset carries a bitmap for. Both are asserted rather than trusted.
fn a_wide_emoji(face: &Face<'_>) -> char {
    let watch = Codepoint::new(0x231A).unwrap();
    assert!(face.glyph(watch).is_some(), "the subset maps U+231A");
    char::from_u32(watch.value()).unwrap()
}

#[test]
fn a_blocks_edge_never_cuts_a_wide_character_in_two() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let noto = font("NotoColorEmoji-subset.ttf");
    let faces = [Face::parse(&mono).unwrap(), Face::parse(&noto).unwrap()];
    let e = a_wide_emoji(&faces[1]);
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    // The emoji straddles the block's left edge on row 1 (lead 1, tail 2)
    // and its right edge on row 2 (lead 2, tail 3); the corners are narrow.
    feed(
        &mut t,
        format!("\x1b[?25labcdefgh\r\na{e}bc\r\nab{e}c\r\nabcdefgh").as_bytes(),
    );
    let view = t.viewport();
    assert!(view.row(Row::from_index(1))[2].content.is_wide_tail());
    assert!(view.row(Row::from_index(2))[3].content.is_wide_tail());
    r.set_highlight(Some(Highlight {
        from: (Row::ZERO, Col::at_index(2)),
        to: (Row::from_index(3), Col::at_index(2)),
        region: Region::Block,
    }));
    let _ = r.convert(&t, Rows::ZERO, Damage::Full);
    let bg = r.background();
    let at = |row: usize, col: usize| bg[row * usize::from(WS.ws_col) + col];
    // The corner at (0, 2) is swapped and (0, 8), never written and outside
    // the block, is not: those are the two words every cell is one of.
    let (on, off) = (at(0, 2), at(0, 8));
    assert_ne!(on, off, "a selected cell's background swaps");
    let swapped = |row: usize, col: usize| {
        let w = at(row, col);
        assert!(w == on || w == off, "({row}, {col}) is neither word");
        w == on
    };
    // Row 1: the lead is outside, so its tail inside the geometry is out.
    assert!(!swapped(1, 1));
    assert!(
        !swapped(1, 2),
        "a tail whose lead is outside is not selected"
    );
    // Row 2: the lead is inside, so its tail past the edge is in.
    assert!(swapped(2, 2));
    assert!(swapped(2, 3), "a tail whose lead is inside is selected");
    // A block is a rectangle: a middle row's cells past its columns are out.
    assert!(!swapped(1, 4) && !swapped(2, 5));
    assert!(swapped(3, 2) && !swapped(3, 3));
}

/// **A selected box-drawing line's background is the floored text, as a
/// selected letter's is** (the owner's call of 2026-10-17, decisions
/// 702/703). A swap hands the background the solid text, `Resolved::fg`,
/// which the floor lifted; the line rule draws a line's glyph as asked and
/// leaves that alone. herdr's divider `#15161b` reads 1.05:1 on the
/// default dark page, so a selection drawn in it as asked would cut a dark
/// gap through the band. Row 0 holds `x` and `U+2502` unselected, which
/// says where each glyph inks; row 1 holds them again, selected. On the
/// pixels neither glyph inks, the selected line's equal the selected
/// letter's with the floor on, and differ from its own with the floor off.
#[test]
fn a_selected_line_takes_the_floored_background_a_letter_does() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut frames = Vec::new();
    let mut words = Vec::new();
    for floor in [Floor::DEFAULT, Floor::Off] {
        let mut r = Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme {
                floor,
                ..Theme::dark()
            },
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap();
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(
            &mut t,
            "\x1b[?25l\x1b[38;2;21;22;27mx\u{2502}\r\nx\u{2502}".as_bytes(),
        );
        r.set_highlight(Some(Highlight {
            from: (Row::from_index(1), Col::at_index(0)),
            to: (Row::from_index(1), Col::at_index(1)),
            region: Region::Stream,
        }));
        let mut canvas = support::Canvas::for_renderer(&r);
        support::paint(&mut r, &mut t, &mut canvas, Damage::Full);
        let m = r.metrics();
        let (cw, ch) = (
            usize::try_from(m.cell_w.value()).unwrap(),
            usize::try_from(m.cell_h.value()).unwrap(),
        );
        let cell = |row: usize, col: usize| -> Vec<u32> {
            (0..ch)
                .flat_map(|y| (0..cw).map(move |x| (row * ch + y) * canvas.width + col * cw + x))
                .map(|i| canvas.pixels[i])
                .collect()
        };
        // The page, from a cell nothing was written to.
        frames.push([cell(0, 0), cell(0, 1), cell(1, 0), cell(1, 1), cell(0, 8)]);
        let bg = r.background();
        let at = |row: usize, col: usize| bg[row * usize::from(WS.ws_col) + col];
        words.push((at(1, 0), at(1, 1)));
    }
    // Pixels neither glyph inks with the floor on or off: the page in row
    // 0, under both.
    let page = &frames[0][4];
    let bare: Vec<usize> = (0..page.len())
        .filter(|&i| {
            frames
                .iter()
                .all(|f| f[0][i] == f[4][i] && f[1][i] == f[4][i])
        })
        .collect();
    let [x, line] = [&frames[0][0], &frames[0][1]];
    assert!(
        bare.len() * 4 > page.len(),
        "the control: most of the cell is bare, {} of {}",
        bare.len(),
        page.len()
    );
    assert!(
        x.iter().zip(page).any(|(p, q)| p != q) && line.iter().zip(page).any(|(p, q)| p != q),
        "the control: both glyphs drew"
    );
    let on = |k: usize| bare.iter().map(|&i| frames[0][k][i]).collect::<Vec<_>>();
    let off = |k: usize| bare.iter().map(|&i| frames[1][k][i]).collect::<Vec<_>>();
    assert_eq!(
        on(3),
        on(2),
        "a selected line's background is a selected letter's, floored"
    );
    assert_ne!(
        on(3),
        off(3),
        "the control: the floor moved the selected line's background"
    );
    assert_eq!(words[0].0, words[0].1, "the background words agree");
    assert_ne!(words[0].1, words[1].1, "the words move with the floor");
}
