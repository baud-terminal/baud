//! **The embedder's notice on the CPU path**: a row of its own below the
//! grid, added by `fit` while the notice is set, drawn across the width in
//! the selection's colours over a frame whose rows above it are the grid's
//! untouched, and gone with the row when the notice is cleared.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Codepoint, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Damage, PxSize, Renderer, Stack, Target, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_col: 30,
    ws_row: 6,
};

const LESS: WinSize = WinSize {
    ws_col: 30,
    ws_row: 5,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// One frame at the renderer's size, damage full.
fn frame(r: &mut Renderer<'_, '_>, t: &mut Term<'_>) -> (Vec<u32>, usize, usize) {
    r.fit(t);
    let (w, h) = r.pixel_size();
    let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    {
        let mut target = Target::new(&mut px, w, w, h).unwrap();
        let seal = t.seal_writes();
        r.render(t, seal, &mut target, Damage::Full);
    }
    (
        px,
        usize::try_from(w.value()).unwrap(),
        usize::try_from(h.value()).unwrap(),
    )
}

fn text(s: &str) -> Vec<Codepoint> {
    s.chars().map(Codepoint::from_char).collect()
}

#[test]
fn a_notice_is_a_row_of_its_own_below_the_grid() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(30),
        Rows::of_index(6),
    )
    .unwrap();
    let ch = usize::try_from(r.metrics().cell_h.value()).unwrap();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"one\r\ntwo\r\nthree\r\nfour\r\nfive");
    // The grid alone, at the five rows the embedder gives it with a notice.
    let _ = t.resize(&LESS);
    let (plain, w, h) = frame(&mut r, &mut t);
    assert_eq!(h, 5 * ch, "five rows, no notice");
    assert_eq!(r.notice_rows(), Rows::ZERO);
    assert!(r.notice_bar().is_none());

    r.set_notice(Some(text(" config.toml line 3: \"x\" is not key = value")));
    let (shown, w2, h2) = frame(&mut r, &mut t);
    assert_eq!((w2, h2), (w, 6 * ch), "fit adds the notice's row");
    assert_eq!(r.notice_rows(), Rows::of_index(1));
    let bar = r.notice_bar().expect("the notice's field");
    assert_eq!(bar.row.index(), 5, "the row after the grid's");
    assert_eq!(bar.text.len(), 30, "padded to the width");
    // The grid's rows are drawn as they were.
    assert_eq!(&shown[..5 * ch * w], &plain[..], "the grid untouched");
    // The notice's row: the selection's colours to the far end, not the
    // grid's background, and its text in another colour over them.
    let row = &shown[5 * ch * w..];
    let grid_bg = plain[w - 1];
    let bar_bg = row[w - 1];
    assert_ne!(bar_bg, grid_bg, "the bar reaches the far end");
    assert!(
        row[(ch / 2) * w..(ch / 2 + 1) * w]
            .iter()
            .any(|&p| p != bar_bg),
        "the text is drawn"
    );

    // Cut to the width, never wrapped.
    r.set_notice(Some(text(&"x".repeat(80))));
    let (_, _, h3) = frame(&mut r, &mut t);
    assert_eq!(h3, 6 * ch);
    assert_eq!(r.notice_bar().unwrap().text.len(), 30);

    // Cleared: the row goes, and the frame is the grid's again.
    r.set_notice(None);
    let (gone, _, h4) = frame(&mut r, &mut t);
    assert_eq!(h4, 5 * ch);
    assert_eq!(gone, plain, "nothing of the notice is left");
}
