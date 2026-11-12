//! **A window scrolled back over history the core froze draws each cell in
//! the rendition it was written in** (`baud_render::Lent`): the cell path's
//! copy gives a frozen row's cells slots of the frame's resolved table no
//! other row names, and resolves its generation's words into them.
//!
//! `[[source: dec-std-070]]`'s SGR, page 5-104, note 3: "In no way does the
//! selection of current rendition affect the rendition of characters already
//! entered into the display."

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, PackedCell, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Lend, Lent, PxSize, Renderer, Resolved, Stack, StyleStamp, Theme, Words};
use support::font;

const SMALL: WinSize = WinSize {
    ws_row: 10,
    ws_col: 40,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    for c in bytes.chunks(4096) {
        t.write_at(BytePos::ZERO, c);
        t.feed(BytePos::ZERO, ReadLen::from_index(c.len()));
    }
}

fn renderer<'f>(faces: &'f [Face<'f>]) -> Renderer<'f, 'f> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(SMALL.ws_col)),
        Rows::of_index(usize::from(SMALL.ws_row)),
    )
    .unwrap()
}

/// Every cell of `lines` lines in a truecolour rendition of its own.
fn flood(lines: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for l in 0..lines {
        for x in 0..40 {
            let i = l * 40 + x;
            let b = i.to_le_bytes();
            out.extend(
                format!("\x1b[0;38;2;{};{};9;48;2;9;{};{}mq", b[0], b[1], b[1], b[0]).as_bytes(),
            );
        }
        out.extend(b"\x1b[0m\r\n");
    }
    out
}

/// **The copy of a window over frozen history, lent slots, resolves every
/// cell to what its own row's table names**, and putting the slots back
/// leaves the table as the core's own resolves it. Without the lending a
/// frozen row's ids name the live table's slots: the control.
#[test]
fn a_window_over_frozen_history_draws_each_cell_in_its_rendition() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&SMALL);
    let mut t = Term::new(owned.storage(), &SMALL);
    assert_eq!(t.styles().len(), 1024);
    feed(&mut t, &flood(300));
    assert!(t.frozen().counts.freezes > 0, "the control: it froze");
    let mut table = vec![Resolved::default(); t.styles().len()];
    let mut stamp = StyleStamp::default();
    r.sync_styles(&t, &mut table, &mut stamp);
    let synced = table.clone();
    let back = Rows::of_count(150);
    let view = t.view(back);
    let (cols, rows) = (view.cols().index(), view.rows().index());
    let frozen = (0..rows)
        .filter(|y| !view.styles(Row::from_index(*y)).is_live())
        .count();
    assert!(frozen == rows, "the control: the window is all frozen rows");
    // What each cell is: its word through its row's table, resolved.
    let want: Vec<Vec<Resolved>> = (0..rows)
        .map(|y| {
            let y = Row::from_index(y);
            let s = view.styles(y);
            view.row(y)
                .iter()
                .map(|c| r.resolved_word(&t, t.style_in(s, *c)))
                .collect()
        })
        .collect();
    let mut copy: Vec<PackedCell> = (0..rows)
        .flat_map(|y| view.row(Row::from_index(y)).to_vec())
        .collect();
    // The control: read through the live table alone, cells are wrong.
    let wrong_live = (0..rows * cols)
        .filter(|&i| table[copy[i].style.index()] != want[i / cols][i % cols])
        .count();
    assert!(
        wrong_live > rows * cols / 2,
        "the control: {wrong_live} wrong"
    );
    let mut lent = Lent::default();
    let mut parts: Vec<(&mut [PackedCell], Words<'_>)> = copy
        .chunks_mut(cols)
        .enumerate()
        .map(|(y, c)| (c, Words::Row(view.styles(Row::from_index(y)))))
        .collect();
    let lend = r.lend_frozen(&t, &mut parts, &mut table, &mut stamp, &mut lent);
    assert_eq!(lend, Lend::Rows);
    let wrong = (0..rows * cols)
        .filter(|&i| table[copy[i].style.index()] != want[i / cols][i % cols])
        .count();
    assert_eq!(wrong, 0, "cells drawn in another rendition");
    assert!(lent.lent().value() > 0);
    r.give_back(&mut table, &mut stamp, &mut lent);
    assert!(table == synced, "the slots were not put back");
}

/// The app's provisioning, whose table holds 65,536 renditions.
const BIG: WinSize = WinSize {
    ws_row: 150,
    ws_col: 400,
};

/// A double-width line of letters, each in a foreground and background of
/// its own, then `flood`'s lines: the line in history, frozen.
fn sized_history() -> Vec<u8> {
    let mut out = b"\x1b#6".to_vec();
    for k in 0..20usize {
        let b = (30_000 + k).to_le_bytes();
        let letter = char::from(b'a' + u8::try_from(k).unwrap());
        out.extend(
            format!(
                "\x1b[0;38;2;{};{};200;48;2;7;{};{}m{letter}",
                b[0], b[1], b[1], b[0]
            )
            .as_bytes(),
        );
    }
    out.extend(b"\x1b[0m\r\n");
    out.extend(flood(23));
    out
}

/// The storages a test feeds `sized_history` to, both drawn at `SMALL`'s
/// window: a 1,024-slot table lent generations, and the reference.
fn sized_terms() -> (Owned, Owned) {
    (Owned::new(&SMALL), Owned::new(&BIG))
}

/// The lent copy of the window `back` lines up, as the app's cell path
/// builds it: each row's cells, its frozen cells lent slots of `table`, and
/// its extents with its line rendition.
fn lent_copy(
    r: &mut Renderer<'_, '_>,
    t: &Term<'_>,
    back: Rows,
    (table, stamp, lent): (&mut [Resolved], &mut StyleStamp, &mut Lent),
) -> (Vec<PackedCell>, Vec<baud_domain::RowExtent>) {
    let view = t.view(back);
    let (cols, rows) = (view.cols().index(), view.rows().index());
    let mut copy: Vec<PackedCell> = (0..rows)
        .flat_map(|y| view.row(Row::from_index(y)).to_vec())
        .collect();
    let mut parts: Vec<(&mut [PackedCell], Words<'_>)> = copy
        .chunks_mut(cols)
        .enumerate()
        .map(|(y, c)| (c, Words::Row(view.styles(Row::from_index(y)))))
        .collect();
    assert_eq!(
        r.lend_frozen(t, &mut parts, table, stamp, lent),
        Lend::Rows,
        "the control: the window holds frozen cells"
    );
    let used = (0..rows)
        .map(|y| {
            baud_domain::RowExtent::of(cols)
                .with_rendition(view.rendition(Row::from_index(y)), view.cols())
        })
        .collect();
    (copy, used)
}

/// **A double-width line the core froze, converted for the cell path from
/// the frame's lent copy** ([`Renderer::sized_lines`]): its records are the
/// ones a table that never fills gives, glyph for glyph and colour for
/// colour, since the conversion is lent the slots the frame's table lent
/// (`set_frame_lent`). Read through the live table alone, they are not.
#[test]
fn a_frozen_sized_line_converts_from_the_lent_copy_as_the_reference_does() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let (mut so, mut bo) = sized_terms();
    let mut s = Term::new(so.storage(), &SMALL);
    let mut b = Term::new(bo.storage(), &SMALL);
    let bytes = sized_history();
    feed(&mut s, &bytes);
    feed(&mut b, &bytes);
    assert!(s.style_reclaims().rewrites > 0, "the control: it froze");
    let back = s.looking_back(Rows::of_count(10_000));
    assert_eq!(back, b.looking_back(Rows::of_count(10_000)));
    assert!(
        s.view(back).rendition(Row::ZERO).is_double(),
        "the control: the window's first line is double-width"
    );
    // The reference: the plain view through its own table.
    let mut rr = renderer(&faces);
    let mut bt = vec![Resolved::default(); b.styles().len()];
    rr.sync_styles(&b, &mut bt, &mut StyleStamp::default());
    let mut rows = Vec::new();
    rr.sized_lines(&b, (&b.view(back), &bt), None, None, &mut rows);
    let want: Vec<_> = rows
        .iter()
        .map(|&y| (y, rr.sized_records(y).to_vec()))
        .collect();
    assert!(!want.is_empty(), "the control: the line converted");
    // The frame's lent copy, as the app builds and lends it.
    let mut r = renderer(&faces);
    let mut table = vec![Resolved::default(); s.styles().len()];
    let mut stamp = StyleStamp::default();
    r.sync_styles(&s, &mut table, &mut stamp);
    let mut lent = Lent::default();
    let (copy, used) = lent_copy(&mut r, &s, back, (&mut table, &mut stamp, &mut lent));
    let view = s.view(back);
    let lent_view = baud_domain::Viewport::new(
        &copy,
        &used,
        baud_domain::Stride::of_cells(view.cols().index()),
        view.cols(),
        view.rows(),
        view.rows(),
        baud_domain::RowRot::NONE,
    );
    r.set_frame_lent(&lent);
    r.sized_lines(&s, (&lent_view, &table), None, None, &mut rows);
    let got: Vec<_> = rows
        .iter()
        .map(|&y| (y, r.sized_records(y).to_vec()))
        .collect();
    assert_eq!(got, want, "the frozen line converted otherwise");
    // The control: the core's ids read through the live table.
    let mut rc = renderer(&faces);
    let mut plain = vec![Resolved::default(); s.styles().len()];
    rc.sync_styles(&s, &mut plain, &mut StyleStamp::default());
    rc.sized_lines(&s, (&s.view(back), &plain), None, None, &mut rows);
    let live: Vec<_> = rows
        .iter()
        .map(|&y| (y, rc.sized_records(y).to_vec()))
        .collect();
    assert_ne!(live, want, "the control: the live table names others");
}

/// **The same line leaving the window in a smooth scroll**
/// ([`Renderer::sized_leaving`]): the line just above the window, converted
/// in its own renditions, as the reference converts it.
#[test]
fn a_frozen_sized_line_leaving_the_window_converts_as_the_reference_does() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let (mut so, mut bo) = sized_terms();
    let mut s = Term::new(so.storage(), &SMALL);
    let mut b = Term::new(bo.storage(), &SMALL);
    let bytes = sized_history();
    feed(&mut s, &bytes);
    feed(&mut b, &bytes);
    assert!(s.style_reclaims().rewrites > 0, "the control: it froze");
    let deepest = s.looking_back(Rows::of_count(10_000));
    let back = Rows::of_count(deepest.index() - 1);
    assert!(
        !s.view(deepest).styles(Row::ZERO).is_live(),
        "the control: the leaving line is frozen"
    );
    let (mut r, mut rr) = (renderer(&faces), renderer(&faces));
    let (mut got, mut want) = (Vec::new(), Vec::new());
    r.sized_leaving(&s, back, &mut got);
    rr.sized_leaving(&b, back, &mut want);
    assert!(!want.is_empty(), "the control: the line converted");
    assert_eq!(got, want, "the leaving line converted otherwise");
}
