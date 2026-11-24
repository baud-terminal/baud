//! **A banded terminal draws as one table draws**: each row's cells
//! resolve through their own band's rendition table, so a screen whose rows
//! sit in eight bands paints, pixel for pixel, what the same feed paints in
//! the single mode, whose one table every row shares.

mod support;

use baud_core::{BandRows, BandSlots, Owned, Provision, Term, Written};
use baud_domain::{BytePos, Col, Cols, ReadLen, Row, RowStyles, Rows, StyleWrites, WinSize};
use baud_font::Face;
use baud_render::{
    Blend, Damage, Grain, PxSize, Renderer, Scrolled, Selection, Span, Stack, Theme,
};
use support::{Canvas, font, paint};

const WS: WinSize = WinSize {
    ws_row: 32,
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
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap()
}

/// Every row in two renditions of its own -- a foreground and a
/// background, 64 in all -- the screen filled top to bottom.
fn the_feed() -> Vec<u8> {
    let mut out = b"\x1b[H".to_vec();
    for r in 0..usize::from(WS.ws_row) {
        out.extend(
            format!(
                "\x1b[{};1H\x1b[0;38;2;{};{};200mrow {r:02} \x1b[48;2;30;{};{}mon its own band",
                r + 1,
                r * 7,
                255 - r * 7,
                r * 5,
                100 + r
            )
            .as_bytes(),
        );
    }
    out
}

/// What `owned`'s terminal paints after the feed, from a fresh renderer.
fn painted(owned: &mut Owned, faces: &[Face<'_>]) -> Vec<u32> {
    let mut r = renderer(faces);
    let mut c = Canvas::for_renderer(&r);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &the_feed());
    paint(&mut r, &mut t, &mut c, Damage::Full);
    c.pixels
}

/// **Eight bands of four rows paint what one table paints**: the CPU path,
/// a frame of the whole screen. Breaks: resolve every row through band 0's
/// table, and the rows of bands 1-7 draw band 0's renditions.
#[test]
fn a_banded_screen_paints_as_one_table_does() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let p = Provision::banded_in(&WS, BandRows::of(Rows::of_index(4)), BandSlots(1_024))
        .expect("40x32 is admitted banded");
    let mut banded = Owned::new(p);
    assert_eq!(banded.band_rows().rows().index(), 4, "bands of 4 rows");
    let mut single = Owned::new(&WS);
    let want = painted(&mut single, &faces);
    let got = painted(&mut banded, &faces);
    assert!(
        want.iter().any(|&p| p != want[0]),
        "the control: a frame with something on it"
    );
    let differ = want.iter().zip(&got).filter(|(a, b)| a != b).count();
    assert_eq!(differ, 0, "pixels that differ from the single mode's");
}

/// **A band coming into view resolves its own slots and no other's**: on
/// a screen of 4-row bands with a history, a frame after the first resolves
/// nothing, and a line feed at the bottom, which brings in one band and
/// keeps the rest, resolves exactly the slots that band's log has written.
/// The bases move with it, so a shader's `seg_base` is uploaded again.
/// Breaks: resync every slot of the window on every frame.
#[test]
fn a_band_coming_into_view_resolves_its_own_slots_alone() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let p = Provision::banded_in(&WS, BandRows::of(Rows::of_index(4)), BandSlots(1_024))
        .expect("40x32 is admitted banded");
    let mut owned = Owned::with_history(
        p,
        &baud_domain::CacheGeometry::new(None, None, None, None),
        baud_domain::Depth::of(100),
    );
    let mut t = Term::new(owned.storage(), &WS);
    let mut r = renderer(&faces);
    feed(&mut t, &the_feed());
    let first = r.convert(&t, Rows::ZERO, Damage::Full);
    assert!(
        first.styles_resolved.value() > 0,
        "the first frame resolves"
    );
    let (_, stamp) = r.row_bases();
    let steady = r.convert(&t, Rows::ZERO, Damage::Incremental);
    assert_eq!(steady.styles_resolved.value(), 0, "nothing new");
    assert_eq!(r.row_bases().1, stamp, "no base moved");
    // One line feed at the bottom: the row it opens is in the next band.
    feed(&mut t, b"\x1b[0m\x1b[32;1H\n");
    let RowStyles::Live(bottom) = t.row_styles(Row::from_index(31)) else {
        panic!("a live row")
    };
    let entering = bottom.band();
    let written = match t.band_styles_written(entering, StyleWrites::NEVER) {
        Written::Slots([a, b]) => a.len() + b.len(),
        Written::Every => panic!("a fresh band has its log"),
    };
    let scrolled = r.convert(&t, Rows::ZERO, Damage::Incremental);
    assert_eq!(
        scrolled.styles_resolved.value(),
        u64::try_from(written).unwrap(),
        "band {entering:?}'s written slots, and no other band's"
    );
    assert_ne!(r.row_bases().1, stamp, "the rows moved bands");
}

/// **Paragraphs of three rows, each in three renditions and right-to-left
/// text among them**, more lines than the screen, so the window can look
/// back, and then a prompt, so every row above it is finished and drawn at
/// its words. A paragraph of three rows against bands of four crosses a
/// band's last row, so some display line joins two bands' rows.
fn the_long_feed() -> Vec<u8> {
    let mut out = Vec::new();
    for r in 0..48usize {
        out.extend(
            format!(
                "\x1b[0;38;2;{};{};90mline {r:02} wraps here \
                 \x1b[1;38;2;{};40;40mwordrunningpasttherightedgeofit\
                 \x1b[22;48;2;20;{};60m and on, שלום עולם {r} מה שלומך, until a third row\x1b[0m\r\n",
                (r * 5) % 256,
                255 - (r * 5) % 256,
                (r * 3 + 60) % 256,
                r * 4
            )
            .as_bytes(),
        );
    }
    out.extend(b"\x1b]133;A\x07$ ");
    out
}

/// **What `owned`'s terminal paints `back` lines up after the long feed**,
/// with `over` done to the renderer and the terminal first, and how many
/// of the frame's lines joined two bands.
fn painted_back(
    owned: &mut Owned,
    faces: &[Face<'_>],
    back: Rows,
    over: &dyn Fn(&mut Renderer<'_, '_>, &mut Term<'_>),
) -> (Vec<u32>, u64) {
    let mut r = renderer(faces);
    let mut c = Canvas::for_renderer(&r);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &the_long_feed());
    over(&mut r, &mut t);
    let mut target = c.target();
    r.render_at(&mut t, back, &mut target, Damage::Full);
    let joined = r.convert(&t, back, Damage::Full).lines_joined.value();
    (c.pixels, joined)
}

/// **A banded screen of wrapped, right-to-left text, at the bottom and
/// looking back, paints as one table does**: the paragraphs drawn at their
/// words ([`baud_render::WordWrap`], on by default) and the bidirectional
/// layout read each line's band, a line that joins two bands' rows
/// included, and so do a selection over those lines and the cursor on
/// each of them. The control: each window draws lines that join two bands,
/// and the cursor and the selection change pixels.
/// Breaks: resolve a joined display line through its first band's table.
#[test]
fn wrapped_and_right_to_left_text_paints_as_one_table_does() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let geom = baud_domain::CacheGeometry::new(None, None, None, None);
    let both = |back: Rows, over: &dyn Fn(&mut Renderer<'_, '_>, &mut Term<'_>)| {
        let p = Provision::banded_in(&WS, BandRows::of(Rows::of_index(4)), BandSlots(1_024))
            .expect("40x32 is admitted banded");
        let mut banded = Owned::with_history(p, &geom, baud_domain::Depth::of(100));
        let mut single = Owned::with_history(&WS, &geom, baud_domain::Depth::of(100));
        let (want, none) = painted_back(&mut single, &faces, back, over);
        let (got, joined) = painted_back(&mut banded, &faces, back, over);
        assert_eq!(none, 0, "one band joins nothing");
        assert!(joined > 0, "{back:?} back: some line joins two bands");
        let differ = want.iter().zip(&got).filter(|(a, b)| a != b).count();
        assert_eq!(differ, 0, "{back:?} back: pixels that differ");
        got
    };
    let selected = |back: Rows| {
        move |r: &mut Renderer<'_, '_>, t: &mut Term<'_>| {
            let scrolled = Scrolled::START.advanced(t.looking_back(Rows::of_index(usize::MAX)));
            let top = scrolled.top(back);
            let view = t.view(back);
            let first = view.row(Row::ZERO).to_vec();
            let last = view.row(Row::from_index(24)).to_vec();
            let mut sel = Selection::at(Span::cell(&first, top, Col::at_index(3)), Grain::Cell);
            sel.extend(Span::cell(
                &last,
                scrolled.line(back, Row::from_index(24)),
                Col::at_index(30),
            ));
            r.set_highlight(sel.highlight(top, view.rows(), view.cols()));
        }
    };
    for back in [Rows::ZERO, Rows::of_index(10)] {
        let plain = both(back, &|_, _| {});
        let lit = both(back, &selected(back));
        assert_ne!(plain, lit, "{back:?} back: the selection is drawn");
    }
    let hidden = both(Rows::ZERO, &|_, t| feed(t, b"\x1b[?25l"));
    for y in 1..=24 {
        let at = format!("\x1b[{y};5H");
        let shown = both(Rows::ZERO, &move |_, t| feed(t, at.as_bytes()));
        assert_ne!(shown, hidden, "row {y}: the cursor is drawn");
    }
}

/// **A banded frame meets no frozen row, a resize mid-session
/// included**: the frames lend a frozen cell no slot on a terminal of more
/// bands than one, which is right only while none draws one. The long feed,
/// a narrower and shorter window, more of the feed, the first size again,
/// and frames at the bottom, ten lines back and as far back as the history
/// goes after each: every one meets none (`Converted::frozen_banded`). The
/// control: each window far back shows the history, and the first one
/// draws lines joining two bands.
#[test]
fn a_banded_frame_meets_no_frozen_row_after_a_resize() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let geom = baud_domain::CacheGeometry::new(None, None, None, None);
    let p = Provision::banded_in(&WS, BandRows::of(Rows::of_index(4)), BandSlots(1_024))
        .expect("40x32 is admitted banded");
    let mut owned = Owned::with_history(p, &geom, baud_domain::Depth::of(100));
    let mut t = Term::new(owned.storage(), &WS);
    let mut r = renderer(&faces);
    feed(&mut t, &the_long_feed());
    let first = r.convert(&t, Rows::ZERO, Damage::Full);
    assert!(first.lines_joined.value() > 0, "lines joining two bands");
    let small = WinSize {
        ws_row: 24,
        ws_col: 30,
    };
    let mut frames = |t: &mut Term<'_>, when: &str| {
        for back in [0, 10, usize::MAX].map(Rows::of_count) {
            let c = r.convert(t, back, Damage::Full);
            assert_eq!(c.frozen_banded.value(), 0, "{when}, {back:?} back");
        }
        assert!(
            t.looking_back(Rows::of_count(usize::MAX)).index() > 0,
            "{when}: the history is there to look back on"
        );
    };
    frames(&mut t, "before the resize");
    let _ = t.resize(&small);
    frames(&mut t, "narrower");
    feed(&mut t, &the_long_feed());
    frames(&mut t, "narrower, fed");
    let _ = t.resize(&WS);
    frames(&mut t, "the first size again");
    feed(&mut t, &the_long_feed());
    frames(&mut t, "the first size, fed");
}
