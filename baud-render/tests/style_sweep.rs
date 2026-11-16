//! The renderer's half of the rendition table's sweep.
//!
//! A sweep frees slots no cell names, and a later write can give a freed slot
//! another rendition at a position the renderer's last frame showed in the
//! old one: the cell's word is the same and its colour is not, so row damage
//! cannot see it. The renderer flagged a reclaim on a write past the
//! table's length, which a sweeping table passes without refusing -- and
//! passes at its first sweep, which logs every slot it frees -- so the flag
//! is the core's own count of what it reclaimed (`Term::style_reclaims`).

mod support;

use baud_core::{Owned, Term};
use baud_domain::{
    BytePos, Chan, ColorWord, Cols, ReadLen, Row, Rows, StyleId, StyleWord, WinSize,
};
use baud_font::Face;
use baud_render::{Blend, Damage, PxSize, Renderer, Stack, Theme};
use support::{Canvas, font, paint};

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

fn fg(r: u8, g: u8, b: u8) -> StyleWord {
    StyleWord::DEFAULT.with_fg(ColorWord::rgb(
        Chan::from_u8(r),
        Chan::from_u8(g),
        Chan::from_u8(b),
    ))
}

/// `n` distinct renditions through the one cell at row 9, column 1.
fn churn(n: usize, tag: u8) -> Vec<u8> {
    let mut out = b"\x1b[9;1H".to_vec();
    for i in 0..n {
        out.extend(format!("\x1b[0;38;2;{tag};{};{}mC\x08", (i >> 8) & 255, i & 255).as_bytes());
    }
    out
}

/// **A slot freed and given another rendition at the same position is
/// repainted.** Frame 1 shows `X` at the origin in rendition `R1`, slot `k`;
/// frame 2, 850 renditions later and before any sweep, still does. In one
/// read the origin is overwritten (so nothing names `k`), the table sweeps,
/// and `X` in `R2` -- chosen to land in `k` -- is written at the origin
/// again. The cell's word is what frame 2 drew and the colour is not; the
/// frame must equal a full repaint.
#[test]
fn a_swept_slot_reused_in_place_is_repainted() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut inc = renderer(&faces);
    let mut full = renderer(&faces);
    let mut a = Canvas::for_renderer(&inc);
    let mut b = Canvas::for_renderer(&full);
    let mut owned = Owned::new(&SMALL);
    let mut t = Term::new(owned.storage(), &SMALL);
    let len = t.styles().one().len();
    assert_eq!(len, 1024);
    feed(&mut t, b"\x1b[H\x1b[0;38;2;200;10;10mX");
    let k = t.row(Row::from_index(0)).expect("a row")[0].style;
    paint(&mut inc, &mut t, &mut a, Damage::Incremental);
    feed(&mut t, &churn(850, 1));
    assert_eq!(t.style_reclaims().sweeps, 0, "the control: no sweep yet");
    paint(&mut inc, &mut t, &mut a, Damage::Incremental);
    // With no frame between, and fewer table writes than the log holds, so
    // the renderer replays the log rather than lapping it and resolving
    // every slot.
    feed(&mut t, b"\x1b[H\x1b[0mX");
    feed(&mut t, &churn(100, 2));
    let r = t.style_reclaims();
    assert!(r.sweeps >= 1, "positive control: the churn swept: {r:?}");
    assert_eq!(r.refusals, 0);
    assert_eq!(
        t.styles().one().slots()[k.index()],
        None,
        "the sweep freed k"
    );
    let (g, b2) = (0..=255u8)
        .flat_map(|g| (0..=255u8).map(move |b| (g, b)))
        .find(|&(g, b)| StyleId::bucket(fg(10, g, b), len) == k.index())
        .expect("a colour whose bucket is k");
    // The cursor goes back where frame 2 left it, so the origin's row is
    // not repainted for the cursor's sake.
    feed(
        &mut t,
        format!("\x1b[H\x1b[0;38;2;10;{g};{b2}mX\x1b[9;1H").as_bytes(),
    );
    assert_eq!(
        t.row(Row::from_index(0)).expect("a row")[0].style,
        k,
        "R2 took k"
    );
    paint(&mut inc, &mut t, &mut a, Damage::Incremental);
    paint(&mut full, &mut t, &mut b, Damage::Full);
    assert!(a.pixels == b.pixels, "the origin kept R1's colour");
}

/// **A sweeping table past its length repaints only what changed.** Once
/// the table has taken more writes than it has slots, a read that mints one
/// rendition into a free slot -- no sweep, no refusal -- changes one row,
/// and the frame dirties one row, where a flag from writes past the length
/// would repaint them all.
#[test]
fn a_mint_past_the_length_without_a_reclaim_dirties_one_row() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&SMALL);
    let mut t = Term::new(owned.storage(), &SMALL);
    let len = t.styles().one().len();
    feed(&mut t, &churn(len + 200, 2));
    assert!(
        t.style_writes().past(len),
        "positive control: past the length"
    );
    paint(&mut r, &mut t, &mut c, Damage::Incremental);
    // The cursor's own row is dirtied by the write, so the text goes on the
    // cursor's row.
    let before = t.style_reclaims();
    feed(&mut t, b"\x1b[0;38;2;77;77;77mQ");
    assert_eq!(t.style_reclaims(), before, "no sweep, no refusal");
    let f = paint(&mut r, &mut t, &mut c, Damage::Incremental);
    assert_eq!(f.rows_dirty.value(), 1, "{f:?}");
}
