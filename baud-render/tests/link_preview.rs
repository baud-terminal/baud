//! **The link preview on the CPU path**: the link's cells underlined and
//! its address in a bar, drawn over a frame already on screen, moved
//! without a trace and gone without a trace.

mod support;

use baud_core::{CacheGeometry, Depth, Owned, Term};
use baud_domain::ImageBudget;
use baud_domain::{BytePos, Codepoint, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_font::Px;
use baud_render::{
    Blend, Damage, Field, FindLook, Hints, Hover, LinkLook, LinkSpan, Look, PxSize, Renderer,
    Scroll, Scroller, Stack, Target, Theme, Transition, Widths, Window,
};
use support::font;

const WS: WinSize = WinSize {
    ws_col: 30,
    ws_row: 6,
};

/// A real 64x48 ImageMagick Sixel, over the text at the top left.
const SWIRL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tests/recordings/sixel/swirl.raw"
));

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn paint(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, px: &mut [u32], damage: Damage) {
    let (w, h) = r.pixel_size();
    let mut target = Target::new(px, w, w, h).unwrap();
    let seal = t.seal_writes();
    r.render(t, seal, &mut target, damage);
}

fn look(bar_row: usize) -> LinkLook {
    let text: Vec<Codepoint> = " https://example.org/ "
        .chars()
        .map(Codepoint::from_char)
        .collect();
    let n = text.len();
    LinkLook {
        spans: vec![LinkSpan {
            row: Row::from_index(2),
            from: Col::at_index(3),
            to: Col::at_index(8),
        }],
        bar: Field {
            row: Row::from_index(bar_row),
            col: Col::ZERO,
            text,
            hints: vec![Hints::SELECTION; n],
            caret: Col::at_index(n),
        },
    }
}

/// Perturbation: drop the bar's or the underline's rows from the damage
/// (`linked` in `convert`) -- the incremental frames keep what the last
/// one drew, and the assertions after a move or a clear fail.
#[test]
fn the_link_preview_is_drawn_moved_and_cleared_without_a_trace() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let room = WinSize {
        ws_col: 40,
        ws_row: 10,
    };
    let mut owned = Owned::with_history(&room, &CacheGeometry::UNKNOWN, Depth::of(10));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l\r\n\r\nsee the guide here\r\nmore text");
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let (w, h) = r.pixel_size();
    let n = usize::try_from(w.value() * h.value()).unwrap();
    let mut plain = vec![0u32; n];
    paint(&mut r, &mut t, &mut plain, Damage::Full);
    let m = r.metrics();
    let (cw, ch) = (
        usize::try_from(m.cell_w.value()).unwrap(),
        usize::try_from(m.cell_h.value()).unwrap(),
    );
    let stride = usize::try_from(w.value()).unwrap();
    let cell = move |row: usize, col: usize| {
        (row * ch..(row + 1) * ch)
            .flat_map(move |y| (col * cw..(col + 1) * cw).map(move |x| y * stride + x))
    };
    // Drawn incrementally over the plain frame, and as a full frame.
    let mut shown = plain.clone();
    r.set_link(Some(look(5)));
    paint(&mut r, &mut t, &mut shown, Damage::Incremental);
    let differs = |a: &[u32], row: usize, col: usize| cell(row, col).any(|i| a[i] != plain[i]);
    assert!(
        differs(&shown, 2, 3) && differs(&shown, 2, 8),
        "the link's cells are underlined"
    );
    assert!(
        !differs(&shown, 2, 2) && !differs(&shown, 2, 9),
        "the cells either side are not"
    );
    assert!(
        differs(&shown, 5, 0) && differs(&shown, 5, 21),
        "the bar covers its cells"
    );
    assert!(!differs(&shown, 5, 22), "and stops at its end");
    // The underline is the foreground's line under the glyphs: the cell's
    // underline rows change and its top rows do not.
    let top = |row: usize, col: usize| {
        (row * ch..row * ch + 2)
            .flat_map(move |y| (col * cw..(col + 1) * cw).map(move |x| y * stride + x))
    };
    assert!(
        top(2, 4).all(|i| shown[i] == plain[i]),
        "the glyph's top is untouched"
    );
    let mut full = vec![0u32; n];
    paint(&mut r, &mut t, &mut full, Damage::Full);
    assert!(full == shown, "an incremental frame is the full one");
    // Moved to the top row: nothing left on the bottom.
    r.set_link(Some(look(0)));
    paint(&mut r, &mut t, &mut shown, Damage::Incremental);
    assert!(
        !differs(&shown, 5, 0),
        "nothing of the bar left where it was"
    );
    assert!(differs(&shown, 0, 0), "drawn where it went");
    // Gone: the frame is the plain one again.
    r.set_link(None);
    paint(&mut r, &mut t, &mut shown, Damage::Incremental);
    assert!(shown == plain, "nothing of the preview is left behind");
}

/// The terminal under the image test: text on its top rows, no cursor,
/// and with `swirl` the Sixel at the top right, over rows 0 to 2.
fn pictured(owned: &mut Owned, swirl: bool) -> Term<'_> {
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25lsome text under the picture\r\nand more");
    if swirl {
        feed(&mut t, b"\x1b[1;23H");
        feed(&mut t, SWIRL);
    }
    t
}

/// **baud's own overlays are drawn over a program's image**: the link
/// preview's bar, the find field, the scrollbar's thumb and the screen
/// fade are the terminal's, and an image a program placed over the text
/// under the bar would hide the address it shows, or show one of its own
/// in its place. With the swirl at the top right, each overlay over it
/// changes the image's pixels, on the frame that draws it and on the
/// next, which repaints nothing and blits no image, so the overlay stays
/// over it; the bar's and the field's cells are what they are with no
/// image; and an incremental frame is the full one. Each case stops at its
/// first failure, and every case's is asserted once, so one case failing
/// leaves the others run. Perturbations: blit every image over the text
/// over the whole frame on every frame again (the thumb, which no
/// `baud-render/tests/image.rs` arm draws, is the steady frame's case
/// here), or blit the images after the overlays -- each fails here.
#[test]
fn baud_s_own_overlays_are_drawn_over_an_image() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let room = WinSize {
        ws_col: 40,
        ws_row: 10,
    };
    let renderer = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let owned = || {
        Owned::with_history(&room, &CacheGeometry::UNKNOWN, Depth::of(10))
            .with_images(ImageBudget::of(64 * 48 + 64))
    };
    let (mut bare, mut with) = (owned(), owned());
    let mut t0 = pictured(&mut bare, false);
    let mut t = pictured(&mut with, true);
    // The bar and the field on row 0 from column 8 to the last, over the
    // image's left half.
    let mut bar = look(0);
    bar.bar.col = Col::at_index(8);
    bar.bar.caret = Col::at_index(30);
    let field = bar.bar.clone();
    let find = FindLook {
        marks: Vec::new(),
        field: field.clone(),
    };
    let thumb = Scroll {
        window: Window {
            retained: Rows::of_count(20),
            back: Rows::of_count(20),
            rows: Rows::of_index(usize::from(WS.ws_row)),
        },
        look: Look {
            shown: baud_domain::Chan::from_u8(255),
            hover: Hover::Rest,
        },
        widths: Widths {
            rest: Px::of(12),
            hovered: Px::of(22),
            inset: Px::of(6),
            strip: Px::of(34),
        },
        scroller: Scroller::Overlay,
    };
    let cases: [(&str, Set, bool); 4] = [
        (
            "the link bar",
            Box::new(move |r, _| r.set_link(Some(bar.clone()))),
            true,
        ),
        (
            "the find field",
            Box::new(move |r, t| r.set_find(t, Some(find.clone()))),
            true,
        ),
        (
            "the thumb",
            Box::new(move |r, _| r.set_scrollbar(Some(thumb))),
            false,
        ),
        (
            "the fade",
            Box::new(|r, _| r.set_screen_fade(Some(Transition::fade(0.25)))),
            false,
        ),
    ];
    let mut wrong = Vec::new();
    for case in &cases {
        let (mut r0, mut r) = (renderer(), renderer());
        let got = overlay_case(case, (&mut r0, &mut r), (&mut t0, &mut t));
        if let Err(e) = got {
            wrong.push(e);
        }
    }
    assert!(wrong.is_empty(), "{wrong:?}");
}

/// What sets an overlay on a renderer for its next frame.
type Set = Box<dyn Fn(&mut Renderer<'_, '_>, &Term<'_>)>;

/// `Ok` when `ok`, else the failure `what` says.
fn check(ok: bool, what: impl FnOnce() -> String) -> Result<(), String> {
    match ok {
        true => Ok(()),
        false => Err(what()),
    }
}

/// [`baud_s_own_overlays_are_drawn_over_an_image`]'s case: its name
/// `what`, its overlay `set`, and whether the overlay's cells are its own,
/// drawn by `r0` on `t0`, the screen with no image, and by `r` on `t`, the
/// one with the swirl: its first failure, if any.
fn overlay_case(
    case: &(&str, Set, bool),
    (r0, r): (&mut Renderer<'_, '_>, &mut Renderer<'_, '_>),
    (t0, t): (&mut Term<'_>, &mut Term<'_>),
) -> Result<(), String> {
    let (what, set, cells) = (case.0, &case.1, case.2);
    let (w, h) = r.pixel_size();
    let n = usize::try_from(w.value() * h.value()).unwrap();
    let stride = usize::try_from(w.value()).unwrap();
    // The overlay with no image under it.
    let mut want = vec![0u32; n];
    set(&mut *r0, &*t0);
    paint(r0, t0, &mut want, Damage::Full);
    // The image alone, then the overlay over it, incrementally.
    let mut shown = vec![0u32; n];
    paint(r, t, &mut shown, Damage::Full);
    let alone = shown.clone();
    let p = r.image_placements()[0];
    let rect = |px: &[u32]| -> Vec<u32> {
        let (x, y) = (usize::try_from(p.x).unwrap(), usize::try_from(p.y).unwrap());
        (y..y + usize::from(p.h))
            .flat_map(|y| (x..x + usize::from(p.w)).map(move |x| y * stride + x))
            .map(|i| px[i])
            .collect()
    };
    set(&mut *r, &*t);
    paint(r, t, &mut shown, Damage::Incremental);
    check(rect(&shown) != rect(&alone), || {
        format!("{what} is drawn over the image")
    })?;
    if cells {
        let m = r.metrics();
        let (cw, ch) = (
            usize::try_from(m.cell_w.value()).unwrap(),
            usize::try_from(m.cell_h.value()).unwrap(),
        );
        let row = |px: &[u32]| -> Vec<u32> {
            (0..ch)
                .flat_map(|y| (8 * cw..30 * cw).map(move |x| y * stride + x))
                .map(|i| px[i])
                .collect()
        };
        check(row(&alone) != row(&want), || format!("{what}: the control"))?;
        check(row(&shown) == row(&want), || {
            format!("{what}'s cells are its own")
        })?;
    }
    // The next frame changes nothing, repaints nothing and blits no image,
    // so the overlay stays over it.
    let before = shown.clone();
    paint(r, t, &mut shown, Damage::Incremental);
    check(shown == before, || format!("{what} stays over the image"))?;
    let mut full = vec![0u32; n];
    paint(r, t, &mut full, Damage::Full);
    check(full == shown, || {
        format!("{what}: an incremental frame is the full one")
    })
}
