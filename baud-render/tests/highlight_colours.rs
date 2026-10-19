//! The selection in `OSC 17`'s and `OSC 19`'s colours, on the CPU path,
//! where the pixel oracle cannot reach: the oracle holds the instance arm
//! and the cell shader to each other, so a rule both lost would pass it.
//!
//! `[[source: ctlseqs]]`: "Ps = 1 7 -> Change highlight background color to
//! Pt", "Ps = 1 9 -> Change highlight foreground color to Pt", and 117 and
//! 119 reset them. Each is drawn whenever it is set (Jev's lean, decisions
//! of 2026-10-18 under the bar, kitty's practice): the page alone draws
//! each cell's own text on it, the text alone draws on the reversed page,
//! and neither is reverse video.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_render::{Argb, Blend, Damage, Highlight, PxSize, Region, Renderer, Stack, Target, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_col: 6,
    ws_row: 2,
};
const RED: Argb = Argb(0xFF_CD_00_00);
const PAGE: Argb = Argb(0xFF_2F_5F_9F);
const TEXT: Argb = Argb(0xFF_FF_D7_5F);

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// A full block in red, so its middle pixel is the text colour, and a
/// blank, selected together, painted under `osc`: the page of each and the
/// middle pixel of the block.
fn selected(osc: &[u8]) -> (Argb, Argb, u32) {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(6),
        Rows::of_index(2),
    )
    .unwrap();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l\x1b[38;2;205;0;0m\xe2\x96\x88\x1b[0m ");
    feed(&mut t, osc);
    r.set_highlight(Some(Highlight {
        from: (Row::ZERO, Col::at_index(0)),
        to: (Row::ZERO, Col::at_index(1)),
        region: Region::Stream,
    }));
    let (w, h) = r.pixel_size();
    let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    {
        let mut target = Target::new(&mut px, w, w, h).unwrap();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let m = r.metrics();
    let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
    let middle = px[usize::try_from((ch / 2) * w.value() + cw / 2).unwrap()];
    (r.background()[0], r.background()[1], middle)
}

#[test]
fn a_selection_is_drawn_in_the_highlight_colours_each_when_set() {
    let dark = Theme::dark();
    let opaque = |c: Argb| Argb(c.0 | 0xFF00_0000);
    // Reverse video: the block's page is its red text, its glyph the page.
    let (block, blank, glyph) = selected(b"");
    assert_eq!(block, RED, "reverse video");
    assert_eq!(blank, opaque(dark.fg), "a blank's page is the text colour");
    assert_eq!(glyph, opaque(dark.bg).0, "the glyph is the page");
    // `OSC 17` alone: each cell's own text on the highlight.
    let (block, blank, glyph) = selected(b"\x1b]17;#2f5f9f\x07");
    assert_eq!((block, blank), (PAGE, PAGE));
    assert_eq!(glyph, RED.0, "the block keeps its own text");
    // `OSC 19` alone: the highlight text on the reversed page.
    let (block, blank, glyph) = selected(b"\x1b]19;#ffd75f\x07");
    assert_eq!((block, blank), (RED, opaque(dark.fg)));
    assert_eq!(glyph, TEXT.0);
    // Both.
    let (block, _, glyph) = selected(b"\x1b]17;#2f5f9f\x1b\\\x1b]19;#ffd75f\x1b\\");
    assert_eq!((block, glyph), (PAGE, TEXT.0));
    // **Carried on**: `OSC 17 ; page ; ; text` sets 17, passes 18, sets 19.
    let (block, _, glyph) = selected(b"\x1b]17;#2f5f9f;;#ffd75f\x07");
    assert_eq!((block, glyph), (PAGE, TEXT.0));
    // `OSC 117` and `119` put reverse video back.
    let (block, _, glyph) = selected(b"\x1b]17;#2f5f9f;;#ffd75f\x07\x1b]117\x07\x1b]119\x07");
    assert_eq!((block, glyph), (RED, opaque(dark.bg).0));
}

/// **A change of the selection's colours repaints the selected rows**,
/// though no cell, rendition or selection moved: an incremental frame after
/// an `OSC 17` draws what a full repaint draws.
#[test]
fn an_incremental_frame_after_osc_17_equals_a_full_repaint() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let make = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(6),
            Rows::of_index(2),
        )
        .unwrap()
    };
    let sel = Some(Highlight {
        from: (Row::ZERO, Col::at_index(0)),
        to: (Row::ZERO, Col::at_index(3)),
        region: Region::Stream,
    });
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25labc");
    let (mut inc, mut full) = (make(), make());
    let (w, h) = inc.pixel_size();
    let n = usize::try_from(w.value() * h.value()).unwrap();
    let (mut a, mut b) = (vec![0u32; n], vec![0u32; n]);
    inc.set_highlight(sel);
    inc.render(
        &mut t,
        &mut Target::new(&mut a, w, w, h).unwrap(),
        Damage::Full,
    );
    let before = a.clone();
    for osc in [
        &b"\x1b]17;#2f5f9f\x07"[..],
        b"\x1b]19;#ffd75f\x07",
        b"\x1b]117\x07",
    ] {
        feed(&mut t, osc);
        inc.render(
            &mut t,
            &mut Target::new(&mut a, w, w, h).unwrap(),
            Damage::Incremental,
        );
        full.set_highlight(sel);
        full.render(
            &mut t,
            &mut Target::new(&mut b, w, w, h).unwrap(),
            Damage::Full,
        );
        assert_eq!(a, b, "after {osc:?}");
    }
    assert_ne!(a, before, "the colours reached the pixels");
}
