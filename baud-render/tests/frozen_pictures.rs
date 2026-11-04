//! **A picture in history the core froze is found in its own colours**
//! (`baud_render::Pictures`), on the core's view and on the cell path's lent
//! copy: each cell's background is read through the table its id names,
//! its row's generation or the words its lent slot was given
//! (`baud_render::Words`). Picture finding is on by default
//! (`[glass] app_backgrounds = "dim"`, `pictures = "opaque"`), and a
//! picture not found draws its backdrop dimmed rather than opaque.
//!
//! `[[source: dec-std-070]]`'s SGR, page 5-104, note 3: "In no way does the
//! selection of current rendition affect the rendition of characters already
//! entered into the display."

mod support;

use baud_core::{Owned, Term};
use baud_domain::{
    BytePos, Cols, PackedCell, ReadLen, Row, RowExtent, RowRot, Rows, Video, WinSize,
};
use baud_font::Face;
use baud_render::{
    Blend, Lend, Lent, Pictures, PxSize, Renderer, Resolved, Stack, StyleStamp, Theme, Words,
};

const WS: WinSize = WinSize {
    ws_row: 10,
    ws_col: 40,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    for c in bytes.chunks(4096) {
        t.write_at(BytePos::ZERO, c);
        t.feed(BytePos::ZERO, ReadLen::from_index(c.len()));
    }
}

/// `lines` lines, every cell its own truecolour background, from `first`.
fn flood(first: usize, lines: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for l in 0..lines {
        for x in 0..40 {
            let i = first + l * 40 + x;
            let b = i.to_le_bytes();
            out.extend(format!("\x1b[0;48;2;{};{};77mq", b[0], b[1]).as_bytes());
        }
        out.extend(b"\x1b[0m\r\n");
    }
    out
}

/// **The core's view of the history picture**, before the table passes 7/8
/// and after: 800 renditions, then 120 more, in a 1,024-slot table.
#[test]
fn a_picture_in_history_is_found_after_the_table_passes_seven_eighths() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &flood(0, 20));
    let before = Pictures::new().find(&t, &t.view(Rows::of_count(10)), Video::Normal);
    feed(&mut t, &flood(800, 3));
    assert!(t.style_reclaims().rewrites > 0, "the control: it froze");
    let after = Pictures::new().find(&t, &t.view(Rows::of_count(13)), Video::Normal);
    assert_eq!(before.value(), 400, "the control: the picture found before");
    assert_eq!(before, after, "the history's picture found differently");
}

/// **The cell path's lent copy**, lent slots as `baud-app` lends them: the
/// picture found on it as on the core's view, and every lent cell's word,
/// read where its slot names it, the word its original cell names through
/// its row's table. Read through the live table alone, the lent cells name
/// other renditions: the control.
#[test]
fn the_cell_paths_lent_copy_finds_the_history_picture() {
    let data = support::font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(40),
        Rows::of_index(10),
    )
    .unwrap();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &flood(0, 20));
    feed(&mut t, &flood(800, 3));
    assert!(t.style_reclaims().rewrites > 0, "the control: it froze");
    let mut table = vec![Resolved::default(); t.styles().len()];
    let mut stamp = StyleStamp::default();
    r.sync_styles(&t, &mut table, &mut stamp);
    let view = t.view(Rows::of_count(13));
    let (cols, rows) = (view.cols().index(), view.rows().index());
    let stride = t.stride().index();
    let mut copy = vec![PackedCell::EMPTY; rows * stride];
    for y in 0..rows {
        copy[y * stride..y * stride + cols].copy_from_slice(&view.row(Row::from_index(y))[..cols]);
    }
    let mut lent = Lent::default();
    let lend = {
        let mut parts: Vec<(&mut [PackedCell], Words<'_>)> = copy
            .chunks_mut(stride)
            .enumerate()
            .map(|(y, c)| (&mut c[..cols], Words::Row(view.styles(Row::from_index(y)))))
            .collect();
        r.lend_frozen(&t, &mut parts, &mut table, &mut stamp, &mut lent)
    };
    assert_eq!(lend, Lend::Rows);
    let used: Vec<RowExtent> = (0..rows).map(|_| RowExtent::of(cols)).collect();
    let lent_view = baud_domain::Viewport::new(
        &copy,
        &used,
        t.stride(),
        view.cols(),
        view.rows(),
        view.rows(),
        RowRot::NONE,
    );
    let pairs: Vec<_> = (0..rows)
        .flat_map(|y| {
            let y = Row::from_index(y);
            let s = view.styles(y);
            view.row(y)[..cols]
                .iter()
                .zip(&lent_view.row(y)[..cols])
                .map(move |(o, l)| (s, *o, *l))
                .collect::<Vec<_>>()
        })
        .collect();
    let live_wrong = pairs
        .iter()
        .filter(|(s, o, l)| t.style_of(*l) != t.style_in(*s, *o))
        .count();
    assert_eq!(live_wrong, 400, "the control: the live table names others");
    let wrong = pairs
        .iter()
        .filter(|(s, o, l)| lent.word(&t, *l) != t.style_in(*s, *o))
        .count();
    assert_eq!(wrong, 0, "a lent cell's word read otherwise");
    let found = Pictures::new().find_lent(&t, &lent_view, &lent, Video::Normal);
    assert_eq!(found.value(), 400, "the picture on the lent copy");
}
