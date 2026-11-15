//! **The kept frame's cursor row is repainted where the grid is**: with
//! the wrap marks' margins lent, [`Renderer::render_hidden`] paints the
//! grid's columns past them, and with the activity lights' line at the
//! top, the grid's rows below it, so the kept frame equals the screen
//! drawn with no cursor, as it does with neither.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{
    BytePos, Chan, Cols, HostTime, ImageAlpha, ImageBudget, ImageMotion, ImagePx, KittyId, Pixel,
    ReadLen, Rows, WinSize,
};
use baud_font::{Face, Px};
use baud_render::{Blend, Damage, Leaving, LineEdge, PxSize, Renderer, Stack, Target, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 6,
    ws_col: 20,
};

fn feed(t: &mut Term<'_>, bytes: &str) {
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn renderer<'f, 'a>(faces: &'f [Face<'a>], band: Px, line: Option<LineEdge>) -> Renderer<'f, 'a> {
    let mut r = Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    r.set_margins(band);
    r.set_lights_line(line, 2.0);
    r
}

fn paint(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, px: &mut [u32], d: Damage) {
    r.fit(t);
    r.place_marks(t, Rows::ZERO, Leaving::None, Px::of(0));
    let (w, h) = r.whole_size();
    let mut target = Target::new(px, w, w, h).unwrap();
    r.render(t, &mut target, d);
}

fn check(band: Px, line: Option<LineEdge>) -> Vec<usize> {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        "\x1b[1 q$ ls\r\nbaud-app docs\r\n$ vim main.rs\x1b[3;3H",
    );
    let mut r = renderer(&faces, band, line);
    r.fit(&t);
    let top = match line {
        Some(LineEdge::Top) => Px::of(30),
        _ => Px::of(0),
    };
    assert_eq!(r.grid_top(), top, "the grid's rows start below a top line");
    let (w, h) = r.whole_size();
    let width = usize::try_from(w.value()).expect("a width");
    let height = usize::try_from(h.value()).expect("a height");
    // The screen with no cursor, drawn whole by a renderer of its own.
    feed(&mut t, "\x1b[?25l");
    let mut fresh = renderer(&faces, band, line);
    let mut want = vec![0u32; width * height];
    paint(&mut fresh, &mut t, &mut want, Damage::Full);
    feed(&mut t, "\x1b[?25h");
    let mut live = vec![0u32; width * height];
    paint(&mut r, &mut t, &mut live, Damage::Full);
    assert_ne!(live, want, "positive control: the cursor drawn");
    {
        let mut target = Target::new(&mut live, w, w, h).unwrap();
        r.render_hidden(&t, &mut target);
    }
    (0..height)
        .filter(|&y| live[y * width..(y + 1) * width] != want[y * width..(y + 1) * width])
        .collect()
}

#[test]
fn render_hidden_without_margins_is_the_screen_with_no_cursor() {
    assert_eq!(check(Px::of(0), None), Vec::<usize>::new());
}

#[test]
fn render_hidden_with_margins_is_the_screen_with_no_cursor() {
    assert_eq!(check(Px::of(16), None), Vec::<usize>::new());
}

/// **With the lights' line at the top, the cursor's rows are repainted
/// below it**, a line's height down, with and without margins; with the
/// line at the bottom the grid's rows start at the top as before.
/// Perturbation: paint the kept rows into the whole frame from its top,
/// and the top-line cases go red: the cursor is still drawn a line down
/// and the rows a line up are painted over.
#[test]
fn render_hidden_with_the_lights_line_is_the_screen_with_no_cursor() {
    for line in [LineEdge::Top, LineEdge::Bottom] {
        for band in [Px::of(0), Px::of(16)] {
            assert_eq!(
                check(band, Some(line)),
                Vec::<usize>::new(),
                "{line:?}, margins {band:?}"
            );
        }
    }
}

/// The partly transparent image's pixel at `(x, y)`: its alpha and colour
/// running across the rect, so a pixel blended twice differs from one
/// blended once nearly everywhere.
fn partial_pixel(x: usize, y: usize) -> Pixel {
    let c = |v: usize| Chan::from_u8(u8::try_from(v % 256).unwrap());
    Pixel::straight(c(x * 7 + y * 13), c(x * 6), c(255 - y * 8), c(90 + x))
}

/// The kept frame's pixel rows that differ from the screen drawn whole
/// with no cursor, a partly transparent image `tall` pixels high put at `at`
/// (a cursor position) and the cursor on row 5, column 2. The premises:
/// the image is partly transparent, on the cursor's row exactly when
/// `on_cursor_row`, and the cursor was drawn.
fn kept_over_partial(at: &str, tall: usize, on_cursor_row: bool) -> Vec<usize> {
    const W: usize = 30;
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(W * tall));
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        &format!("\x1b[1 q$ ls\r\nbaud-app docs\r\n$ vim main.rs{at}"),
    );
    let _ = t.put_image(
        ImagePx::of(u16::try_from(W).unwrap()),
        ImagePx::of(u16::try_from(tall).unwrap()),
        ImageAlpha::Partial,
        |px| {
            for (i, p) in px.iter_mut().enumerate() {
                *p = partial_pixel(i % W, i / W);
            }
        },
    );
    feed(&mut t, "\x1b[6;3H");
    let mut r = renderer(&faces, Px::of(0), None);
    let (w, h) = r.whole_size();
    let width = usize::try_from(w.value()).expect("a width");
    let height = usize::try_from(h.value()).expect("a height");
    feed(&mut t, "\x1b[?25l");
    let mut fresh = renderer(&faces, Px::of(0), None);
    let mut want = vec![0u32; width * height];
    paint(&mut fresh, &mut t, &mut want, Damage::Full);
    feed(&mut t, "\x1b[?25h");
    let mut live = vec![0u32; width * height];
    paint(&mut r, &mut t, &mut live, Damage::Full);
    assert_ne!(live, want, "the premise: the cursor drawn");
    let ch = i64::from(r.metrics().cell_h.value());
    let image = r.image_placements().first().expect("the premise: placed");
    assert_eq!(
        image.alpha,
        ImageAlpha::Partial,
        "the premise: partly transparent"
    );
    let (first, last) = (
        i64::from(image.y).div_euclid(ch),
        (i64::from(image.y) + i64::from(image.h) - 1).div_euclid(ch),
    );
    assert_eq!(
        (first..=last).contains(&5),
        on_cursor_row,
        "the premise: the image, rows {first} to {last}, on the cursor's row 5"
    );
    {
        let mut target = Target::new(&mut live, w, w, h).unwrap();
        r.render_hidden(&t, &mut target);
    }
    (0..height)
        .filter(|&y| live[y * width..(y + 1) * width] != want[y * width..(y + 1) * width])
        .collect()
}

/// **The kept frame leaves a partly transparent image off the cursor's
/// rows as it was drawn** (FS-C8): an image over the text is blitted only
/// into the rows a frame paints, so taking the cursor off -- which paints
/// the cursor's row -- blits nothing over an image two rows above it,
/// where a blit of every image over the whole frame blended it a second
/// time over its own last blit. The kept frame equals the screen drawn
/// whole with no cursor. The premises: the image is partly transparent
/// and on no row the cursor is on, and the cursor was drawn.
#[test]
fn render_hidden_leaves_a_partial_image_off_the_cursors_row_as_drawn() {
    assert_eq!(
        kept_over_partial("\x1b[1;4H", 20, false),
        Vec::<usize>::new(),
        "pixel rows differ"
    );
}

/// **The kept frame blends a partly transparent image on the cursor's row
/// into it once** (FS-C8): the row the cursor is taken off is painted
/// again, and the images over the text that cover it are blitted into it
/// after its glyphs, once, so the image there is what the screen drawn
/// whole with no cursor shows. The premises: the image is partly
/// transparent and on the cursor's row, and the cursor was drawn. Named
/// break: the kept frame blits no image over the text.
#[test]
fn render_hidden_blends_a_partial_image_on_the_cursors_row_once() {
    assert_eq!(
        kept_over_partial("\x1b[6;4H", 18, true),
        Vec::<usize>::new(),
        "pixel rows differ"
    );
}

/// What moves an image's pixels between the last frame and the kept one.
#[derive(Clone, Copy, Debug)]
enum Moved {
    /// The frame shown (`a=a,c=2`), at the same store epoch, as a tick of
    /// the animation clock moves it before the frame it owes.
    Frame,
    /// The frame shown edited in place (`a=f` with `r=1`), which moves the
    /// store's epoch and not the frame shown.
    Epoch,
}

/// The layer a kitty image of [`kept_after`] is drawn in.
#[derive(Clone, Copy, Debug)]
enum Layer {
    /// `z=0`: over the text.
    Over,
    /// `z=-1`: under the text, over the backgrounds.
    Under,
}

/// Where the cursor of [`kept_after`] is on the image's first row when the
/// last frame is drawn.
#[derive(Clone, Copy, Debug)]
enum Cursor {
    /// Right of the image's columns.
    Right,
    /// Inside them: under a partly transparent image over the text, which
    /// shows it through, or over one under the text.
    Inside,
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `data` in base64, padded.
fn b64(data: &[u8]) -> String {
    let mut out = Vec::new();
    for q in data.chunks(3) {
        let mut b = [0u8; 3];
        b[..q.len()].copy_from_slice(q);
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        for k in 0..4 {
            match k <= q.len() {
                true => out.push(B64[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]),
                false => out.push(b'='),
            }
        }
    }
    String::from_utf8(out).unwrap()
}

/// A kitty graphics command: the keys `ctrl`, and `px`'s RGBA pixels as
/// its payload when there are any.
fn kitty(ctrl: &str, px: &[[u8; 4]]) -> String {
    let mut v = format!("\x1b_G{ctrl}");
    if !px.is_empty() {
        v.push(';');
        v.push_str(&b64(&px.concat()));
    }
    v.push_str("\x1b\\");
    v
}

/// The kept frame of a partly transparent kitty image in `layer` whose
/// pixels `moved` after the last frame drew it, the cursor where `cursor`
/// says, against the screen drawn whole with no cursor AFTER they moved: a
/// description of the pixel rows that differ, if any. The premises: the
/// cursor was drawn, on the image's first row, right of its columns or
/// inside them, and the image reaches a row below the cursor's; the image
/// is in `layer`; only `moved`'s key moved, and the pixels moved below the
/// cursor's row. The control: the screen drawn whole after the move
/// differs from the one before it.
fn kept_after(layer: Layer, moved: Moved, cursor: Cursor) -> Option<String> {
    // Partly transparent, so what was drawn under the image -- the cursor
    // among it -- shows through.
    const RED: [u8; 4] = [200, 40, 40, 160];
    const GREEN: [u8; 4] = [40, 200, 40, 160];
    const BLUE: [u8; 4] = [40, 40, 200, 160];
    let arm = format!("{layer:?} {moved:?} {cursor:?}");
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(1 << 16));
    let mut t = Term::new(owned.storage(), &WS);
    let mut r = renderer(&faces, Px::of(0), None);
    let m = r.metrics();
    t.set_cell_size(
        ImagePx::of(u16::try_from(m.cell_w.value()).unwrap()),
        ImagePx::of(u16::try_from(m.cell_h.value()).unwrap()),
    );
    feed(
        &mut t,
        "\x1b[1 q$ ls\r\nbaud-app docs\r\n$ vim main.rs\x1b[2;1H",
    );
    let z = match layer {
        Layer::Over => "z=0",
        Layer::Under => "z=-1",
    };
    feed(
        &mut t,
        &kitty(&format!("a=T,f=32,s=32,v=32,i=1,{z},C=1,q=2"), &[RED; 1024]),
    );
    if matches!(moved, Moved::Frame) {
        // Its second frame, stored before the last frame is drawn.
        feed(&mut t, &kitty("a=f,f=32,s=32,v=32,i=1,q=2", &[GREEN; 1024]));
    }
    feed(
        &mut t,
        match cursor {
            Cursor::Right => "\x1b[2;12H",
            Cursor::Inside => "\x1b[2;2H",
        },
    );
    let (w, h) = r.whole_size();
    let width = usize::try_from(w.value()).expect("a width");
    let height = usize::try_from(h.value()).expect("a height");
    feed(&mut t, "\x1b[?25l");
    let mut fresh = renderer(&faces, Px::of(0), None);
    let mut before = vec![0u32; width * height];
    paint(&mut fresh, &mut t, &mut before, Damage::Full);
    feed(&mut t, "\x1b[?25h");
    let mut live = vec![0u32; width * height];
    paint(&mut r, &mut t, &mut live, Damage::Full);
    assert_ne!(live, before, "{arm}: the premise: the cursor drawn");
    let (cw, ch) = (i64::from(m.cell_w.value()), i64::from(m.cell_h.value()));
    let image = *r.image_placements().first().expect("the premise: placed");
    assert_eq!(
        image.layer.under_text(),
        matches!(layer, Layer::Under),
        "{arm}: the premise: the image in its layer"
    );
    let (first, last) = (
        i64::from(image.y).div_euclid(ch),
        (i64::from(image.y) + i64::from(image.h) - 1).div_euclid(ch),
    );
    assert!(
        first == 1 && last > 1,
        "{arm}: the premise: the image, rows {first} to {last}, from the cursor's row 1 to below it"
    );
    let (x0, x1) = (i64::from(image.x), i64::from(image.x) + i64::from(image.w));
    let placed = match cursor {
        Cursor::Right => x1 <= 11 * cw,
        Cursor::Inside => x0 <= cw && x1 >= 2 * cw,
    };
    assert!(
        placed,
        "{arm}: the premise: the cursor's column against the image's, {x0} to {x1}"
    );
    match moved {
        Moved::Frame => {
            feed(&mut t, &kitty("a=a,i=1,c=2,q=2", &[]));
            let _ = t.animate_images(HostTime::of_nanos(1), ImageMotion::Hold);
        }
        // The whole frame, so every row the image covers moves.
        Moved::Epoch => feed(
            &mut t,
            &kitty("a=f,f=32,s=32,v=32,x=0,y=0,r=1,i=1,q=2", &[BLUE; 1024]),
        ),
    }
    feed(&mut t, "\x1b[?25l");
    let mut now = renderer(&faces, Px::of(0), None);
    let mut after = vec![0u32; width * height];
    paint(&mut now, &mut t, &mut after, Damage::Full);
    feed(&mut t, "\x1b[?25h");
    assert_ne!(after, before, "{arm}: the control: the pixels moved");
    // Below the cursor's row too, where the kept frame paints nothing for
    // the cursor: else a blit into the cursor's rows alone is right.
    let below = usize::try_from(2 * ch).unwrap_or(usize::MAX)..height;
    assert!(
        below
            .clone()
            .any(|y| after[y * width..(y + 1) * width] != before[y * width..(y + 1) * width]),
        "{arm}: the premise: the pixels moved below the cursor's row, {below:?}"
    );
    let p = *now
        .image_placements()
        .first()
        .expect("placed after the move");
    let only = match moved {
        Moved::Frame => p.frame != image.frame && p.epoch == image.epoch,
        Moved::Epoch => p.epoch != image.epoch && p.frame == image.frame,
    };
    assert!(only, "{arm}: the premise: that key alone moved");
    {
        let mut target = Target::new(&mut live, w, w, h).unwrap();
        r.render_hidden(&t, &mut target);
    }
    let wrong: Vec<usize> = (0..height)
        .filter(|&y| live[y * width..(y + 1) * width] != after[y * width..(y + 1) * width])
        .collect();
    (!wrong.is_empty()).then(|| format!("{arm}: pixel rows {wrong:?} differ"))
}

/// `layer`'s arms of [`kept_after`], the frame shown moved and the epoch
/// moved, each with the cursor right of the image and inside its columns:
/// every arm run, the failures collected.
fn arms(layer: Layer) -> Vec<String> {
    [Moved::Frame, Moved::Epoch]
        .into_iter()
        .flat_map(|moved| [Cursor::Right, Cursor::Inside].map(|cursor| (moved, cursor)))
        .filter_map(|(moved, cursor)| kept_after(layer, moved, cursor))
        .collect()
}

/// **The kept frame draws an image over the text whose pixels moved since
/// the last frame in its new pixels, on every row it covers** (FS-C8): the
/// kept frame places nothing, so a placement's frame and epoch are the
/// last frame's while the store may hold other pixels for the image -- the
/// animation clock's next frame, or an edit read, between the last frame
/// and the kept one. Every row such an image covers is repainted, from the
/// last frame's cells, and the image blitted into them in the pixels the
/// store holds: a blit into the cursor's rows alone drew them in the new
/// pixels beside the old on the rest, a seam, and the last frame's pixels
/// put back there showed the cursor through a partly transparent image for
/// as long as the kept frame is shown. The kept frame equals the screen
/// drawn whole with no cursor after the pixels moved, for the frame shown
/// and the epoch moved, the cursor right of the image and inside its
/// columns; each arm is collected and the failures asserted once. Named
/// breaks: the kept frame keeps such an image as the last frame drew it
/// (the pieces); it takes the frame shown for the image's pixels alone.
#[test]
fn render_hidden_draws_an_image_over_the_text_whose_pixels_moved_in_its_new_pixels_on_every_row() {
    let wrong = arms(Layer::Over);
    assert!(wrong.is_empty(), "{wrong:?}");
}

/// **The kept frame draws an image under the text whose pixels moved since
/// the last frame in its new pixels, on every row it covers** (FS-C8):
/// the kept frame repaints the cursor's rows, and an image under the text
/// is drawn under each repainted row's glyphs in the pixels the store
/// holds, so one whose frame or epoch moved since the last frame was drawn
/// new on the cursor's rows and old on the rows the kept frame keeps -- on
/// master as well. Every row it covers is repainted now, as for an image
/// over the text. The kept frame equals the screen drawn whole with no
/// cursor after the pixels moved, in the same four arms, collected. Named
/// break: the rows of an image under the text whose pixels moved not
/// repainted.
#[test]
fn render_hidden_draws_an_image_under_the_text_whose_pixels_moved_in_its_new_pixels_on_every_row() {
    let wrong = arms(Layer::Under);
    assert!(wrong.is_empty(), "{wrong:?}");
}

/// **The kept frame repaints the rows of an image whose pixels moved, and
/// not the rows of another whose pixels did not** (FS-C8): which images'
/// pixels moved is asked of each image, not of the store's one epoch, which
/// any image stored, drawn on or freed moves. Two opaque kitty images, A on
/// rows 0-1 and B on rows 2-3, the cursor on row 5; B's frame edited in
/// place (`a=f` with `r=1`) after the last frame. The rows of both images
/// are written over in the target with a word no frame draws before the
/// kept frame, so a row it paints is told from a row it keeps: B's rows
/// are painted (the control), A's are kept. The premises: the three rows
/// apart, and the edit moved B's pixels alone. Named break: the store's
/// epoch for every image's.
#[test]
fn render_hidden_repaints_the_rows_of_the_image_whose_pixels_moved_alone() {
    const RED: [u8; 4] = [200, 40, 40, 255];
    const GREEN: [u8; 4] = [40, 200, 40, 255];
    const BLUE: [u8; 4] = [40, 40, 200, 255];
    const UNDRAWN: u32 = 0x5a5a_a5a5;
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(1 << 16));
    let mut t = Term::new(owned.storage(), &WS);
    let mut r = renderer(&faces, Px::of(0), None);
    let m = r.metrics();
    t.set_cell_size(
        ImagePx::of(u16::try_from(m.cell_w.value()).unwrap()),
        ImagePx::of(u16::try_from(m.cell_h.value()).unwrap()),
    );
    feed(&mut t, "\x1b[1 q\x1b[1;1H");
    feed(
        &mut t,
        &kitty("a=T,f=32,s=32,v=32,i=1,C=1,q=2", &[RED; 1024]),
    );
    feed(&mut t, "\x1b[3;11H");
    feed(
        &mut t,
        &kitty("a=T,f=32,s=32,v=32,i=2,C=1,q=2", &[BLUE; 1024]),
    );
    feed(&mut t, "\x1b[6;1H");
    let (w, h) = r.whole_size();
    let width = usize::try_from(w.value()).expect("a width");
    let height = usize::try_from(h.value()).expect("a height");
    feed(&mut t, "\x1b[?25l");
    let mut fresh = renderer(&faces, Px::of(0), None);
    let mut before = vec![0u32; width * height];
    paint(&mut fresh, &mut t, &mut before, Damage::Full);
    feed(&mut t, "\x1b[?25h");
    let mut live = vec![0u32; width * height];
    paint(&mut r, &mut t, &mut live, Damage::Full);
    let ch = i64::from(m.cell_h.value());
    let by_age = |k| {
        let mut p = r.image_placements().to_vec();
        p.sort_by_key(|p| p.born);
        *p.get(k).expect("the premise: two placed")
    };
    let (a, b) = (by_age(0), by_age(1));
    let rows = |y: i16, h: u16| {
        (
            i64::from(y).div_euclid(ch),
            (i64::from(y) + i64::from(h) - 1).div_euclid(ch),
        )
    };
    let ((a0, a1), (b0, b1)) = (rows(a.y, a.h), rows(b.y, b.h));
    assert!(
        a1 < b0 && b1 < 5,
        "the premise: A's rows {a0} to {a1}, B's {b0} to {b1} and the cursor's 5 apart"
    );
    feed(
        &mut t,
        &kitty("a=f,f=32,s=8,v=8,x=0,y=0,r=1,i=2,q=2", &[GREEN; 64]),
    );
    feed(&mut t, "\x1b[?25l");
    let mut now = renderer(&faces, Px::of(0), None);
    let mut after = vec![0u32; width * height];
    paint(&mut now, &mut t, &mut after, Damage::Full);
    feed(&mut t, "\x1b[?25h");
    // The pixels of each image's rect, as indices into a frame.
    let rect = |p: baud_render::ImagePlacement| {
        let (x0, y0) = (
            usize::try_from(p.x).unwrap_or(0),
            usize::try_from(p.y).unwrap_or(0),
        );
        let (x1, y1) = (
            (x0 + usize::from(p.w)).min(width),
            (y0 + usize::from(p.h)).min(height),
        );
        (y0..y1).flat_map(move |y| (x0..x1).map(move |x| y * width + x))
    };
    assert!(
        rect(a).all(|i| after[i] == before[i]),
        "the premise: A's pixels did not move"
    );
    assert!(
        rect(b).any(|i| after[i] != before[i]),
        "the premise: B's did"
    );
    for i in rect(a).chain(rect(b)) {
        live[i] = UNDRAWN;
    }
    {
        let mut target = Target::new(&mut live, w, w, h).unwrap();
        r.render_hidden(&t, &mut target);
    }
    assert!(
        rect(b).all(|i| live[i] != UNDRAWN),
        "the control: B's rows painted"
    );
    assert!(rect(a).all(|i| live[i] == UNDRAWN), "A's rows kept");
}

/// Which of [`gone_over_live`]'s two overlapping kitty images is deleted
/// after the last frame and which is drawn over the other, and what becomes
/// of the other, the live one.
#[derive(Clone, Copy, Debug)]
enum Overlap {
    /// The one under deleted and the one over it redrawn (its frame edited
    /// in place, `a=f` with `r=1`): every row the live one covers is
    /// painted, the overlap off the cursor's row. The gone one's piece put
    /// back whole showed the live one's last pixels over its new ones.
    UnderGoneOverRedrawn,
    /// The one over deleted and the one under it redrawn, the overlap off
    /// the cursor's row: the gone one stays over the live one there, as the
    /// last frame drew it, where a clip against the one under it showed the
    /// live one over the gone one.
    OverGoneUnderRedrawn,
    /// The one over deleted and the one under it the same, the overlap on
    /// the cursor's row, which the kept frame paints, and on the row below
    /// it, which the kept frame keeps: the gone one over the live one on
    /// both, where a clip against the one the same showed the live one over
    /// the gone one on the row painted alone.
    OverGoneUnderSame,
    /// The one under deleted and the one over it the same and partly
    /// transparent, the overlap on the cursor's row and on the row below
    /// it: the live one blended over the gone one on both, where a clip
    /// against the live one showed it over the text alone on the row
    /// painted, the gone one vanished through it.
    UnderGoneOverSameTranslucent,
}

/// The kept frame of two overlapping kitty images over the text, one
/// deleted with its data (`a=d,d=I`) after the last frame drew both and
/// the other live, as `case` says, against the kept frame's own rule, on
/// every row of the live one: the gone one as the last frame drew it
/// (`before`, the screen drawn whole with no cursor then) where the two
/// overlap, unless the live one is redrawn over it, and the screen drawn
/// whole with no cursor after (`after`) everywhere else. A description of
/// the live image's pixels that differ, if any, and the cell rows they are
/// on. A gone image is kept from the last frame on the rows painted, in
/// pieces taken before they are painted, so a piece holds what the last
/// frame drew there, the live image among it where the two overlap. The
/// premises: the cursor drawn, its cell off the live image; the two
/// overlap, off the cursor's row when the live one is redrawn, and on the
/// cursor's row and past it, on a row the kept frame keeps, when it is
/// the same; the live one partly transparent in the translucent arm alone,
/// the gone one opaque; the upper one drawn in the overlap, blended there
/// when partly transparent; the overlap drawn otherwise after; the one
/// deleted gone and the other held; the live one's pixels moved exactly
/// when redrawn.
fn gone_over_live(case: Overlap) -> Option<String> {
    const RED: [u8; 4] = [200, 40, 40, 255];
    const GREEN: [u8; 4] = [40, 200, 40, 255];
    const GLASS: [u8; 4] = [40, 200, 40, 160];
    const BLUE: [u8; 4] = [40, 40, 200, 255];
    let arm = format!("{case:?}");
    let redrawn = matches!(
        case,
        Overlap::UnderGoneOverRedrawn | Overlap::OverGoneUnderRedrawn
    );
    let gone_over = matches!(
        case,
        Overlap::OverGoneUnderRedrawn | Overlap::OverGoneUnderSame
    );
    let translucent = matches!(case, Overlap::UnderGoneOverSameTranslucent);
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(1 << 16));
    let mut t = Term::new(owned.storage(), &WS);
    let mut r = renderer(&faces, Px::of(0), None);
    let m = r.metrics();
    t.set_cell_size(
        ImagePx::of(u16::try_from(m.cell_w.value()).unwrap()),
        ImagePx::of(u16::try_from(m.cell_h.value()).unwrap()),
    );
    feed(&mut t, "\x1b[1 q$ ls\r\nbaud-app docs\r\n$ vim main.rs");
    let (gone_z, live_z) = match gone_over {
        true => (1, 0),
        false => (0, 1),
    };
    // Redrawn: the gone one from row 0, the live one from row 1, the
    // cursor on row 5. The same: both from row 1, the cursor on row 1
    // right of both.
    let (gone_at, cursor, cursor_row, cursor_col) = match redrawn {
        true => ("\x1b[1;1H", "\x1b[6;1H", 5, 0),
        false => ("\x1b[2;1H", "\x1b[2;12H", 1, 11),
    };
    feed(&mut t, gone_at);
    feed(
        &mut t,
        &kitty(
            &format!("a=T,f=32,s=32,v=32,i=1,z={gone_z},C=1,q=2"),
            &[RED; 1024],
        ),
    );
    feed(&mut t, "\x1b[2;3H");
    let live_px = match translucent {
        true => GLASS,
        false => GREEN,
    };
    feed(
        &mut t,
        &kitty(
            &format!("a=T,f=32,s=32,v=32,i=2,z={live_z},C=1,q=2"),
            &[live_px; 1024],
        ),
    );
    feed(&mut t, cursor);
    let (w, h) = r.whole_size();
    let width = usize::try_from(w.value()).expect("a width");
    let height = usize::try_from(h.value()).expect("a height");
    feed(&mut t, "\x1b[?25l");
    let mut fresh = renderer(&faces, Px::of(0), None);
    let mut before = vec![0u32; width * height];
    paint(&mut fresh, &mut t, &mut before, Damage::Full);
    feed(&mut t, "\x1b[?25h");
    let mut live = vec![0u32; width * height];
    paint(&mut r, &mut t, &mut live, Damage::Full);
    assert_ne!(live, before, "{arm}: the premise: the cursor drawn");
    let by_age = |k| {
        let mut p = r.image_placements().to_vec();
        p.sort_by_key(|p| p.born);
        *p.get(k).expect("the premise: two placed")
    };
    let (gone, held) = (by_age(0), by_age(1));
    assert_eq!(
        (
            held.alpha == ImageAlpha::Partial,
            gone.alpha == ImageAlpha::Partial
        ),
        (translucent, false),
        "{arm}: the premise: the live one partly transparent in the translucent arm alone, the gone one opaque"
    );
    // A placement's pixel columns and rows in the frame.
    let span = |p: baud_render::ImagePlacement| {
        let (x0, y0) = (
            usize::try_from(p.x).unwrap_or(0),
            usize::try_from(p.y).unwrap_or(0),
        );
        (
            x0..(x0 + usize::from(p.w)).min(width),
            y0..(y0 + usize::from(p.h)).min(height),
        )
    };
    let ((gx, gy), (kx, ky)) = (span(gone), span(held));
    let (ox, oy) = (
        gx.start.max(kx.start)..gx.end.min(kx.end),
        gy.start.max(ky.start)..gy.end.min(ky.end),
    );
    assert!(
        !ox.is_empty() && !oy.is_empty(),
        "{arm}: the premise: the two overlap, {gx:?} by {gy:?} and {kx:?} by {ky:?}"
    );
    let cw = usize::try_from(m.cell_w.value()).expect("a cell width");
    let ch = usize::try_from(m.cell_h.value()).expect("a cell height");
    let meets = |a: &core::ops::Range<usize>, b: &core::ops::Range<usize>| {
        a.start < b.end && b.start < a.end
    };
    let band = cursor_row * ch..(cursor_row + 1) * ch;
    let past = oy.start < band.start || oy.end > band.end;
    assert_eq!(
        (meets(&oy, &band), meets(&oy, &band) && past),
        (!redrawn, !redrawn),
        "{arm}: the premise: the overlap, pixel rows {oy:?}, off the cursor's, {band:?}, when redrawn, on them and past them when the same"
    );
    let cell = cursor_col * cw..(cursor_col + 1) * cw;
    let on_live = meets(&cell, &kx) && meets(&band, &ky);
    assert!(
        !on_live,
        "{arm}: the premise: the cursor's cell, {cell:?} by {band:?}, off the live image"
    );
    let at = |px: &[u32], x: usize, y: usize| px[y * width + x];
    // Each one's own corner, outside the other, and the overlap's.
    let (gone_corner, live_corner) = ((gx.start, gy.start), (kx.end - 1, ky.end - 1));
    let own = |(x, y): (usize, usize)| at(&before, x, y);
    let mid = at(&before, ox.start, oy.start);
    let drawn = match (translucent, gone_over) {
        (true, _) => mid != own(gone_corner) && mid != own(live_corner),
        (false, true) => mid == own(gone_corner),
        (false, false) => mid == own(live_corner),
    };
    assert!(
        drawn,
        "{arm}: the premise: the upper one drawn in the overlap, blended when partly transparent"
    );
    feed(&mut t, &kitty("a=d,d=I,i=1,q=2", &[]));
    if redrawn {
        feed(
            &mut t,
            &kitty("a=f,f=32,s=32,v=32,x=0,y=0,r=1,i=2,q=2", &[BLUE; 1024]),
        );
    }
    {
        let imgs = t.images();
        assert!(
            imgs.kitty_image(KittyId::of(1)).is_none()
                && imgs.kitty_image(KittyId::of(2)).is_some(),
            "{arm}: the premise: the one deleted gone, the other held"
        );
    }
    feed(&mut t, "\x1b[?25l");
    let mut now = renderer(&faces, Px::of(0), None);
    let mut after = vec![0u32; width * height];
    paint(&mut now, &mut t, &mut after, Damage::Full);
    feed(&mut t, "\x1b[?25h");
    assert_ne!(
        mid,
        at(&after, ox.start, oy.start),
        "{arm}: the premise: the overlap drawn otherwise after"
    );
    let (cx, cy) = live_corner;
    assert_eq!(
        at(&after, cx, cy) != at(&before, cx, cy),
        redrawn,
        "{arm}: the premise: the live image's pixels moved exactly when redrawn"
    );
    {
        let mut target = Target::new(&mut live, w, w, h).unwrap();
        r.render_hidden(&t, &mut target);
    }
    // The kept frame's own rule: the gone one as the last frame drew it
    // where the two overlap, unless the live one is redrawn over it.
    let cut = redrawn && !gone_over;
    let wrong: Vec<(usize, usize)> = ky
        .clone()
        .flat_map(|y| kx.clone().map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let want = match ox.contains(&x) && oy.contains(&y) && !cut {
                true => &before,
                false => &after,
            };
            at(&live, x, y) != at(want, x, y)
        })
        .collect();
    let mut rows: Vec<usize> = wrong.iter().map(|&(_, y)| y / ch).collect();
    rows.dedup();
    (!wrong.is_empty()).then(|| {
        format!(
            "{arm}: {} pixels of the live image differ from the kept frame's rule, the first at {:?}, on cell rows {rows:?}",
            wrong.len(),
            wrong[0]
        )
    })
}

/// **The kept frame clips a gone image only to the images redrawn over
/// it** (FS-C8): a gone image's pieces are taken from the last frame
/// before the rows are painted, so where it overlapped a live image a
/// piece holds that image's last pixels, and put back whole over one
/// redrawn over it they showed those over its new ones, for as long as the
/// kept frame is shown. A fresh render draws no gone image, so the kept
/// frame's own rule is the comparator: the gone image as the last frame
/// drew it, on the rows painted and the rows kept alike, and every live
/// image in the pixels it holds now. So a piece is clipped where an image
/// redrawn over it is, and nowhere else: a clip against an image the same
/// showed it through the gone one or over it on the rows painted alone,
/// and a clip against one redrawn under it put the redrawn one over the
/// gone one. Four arms, collected: the one under deleted and the one over
/// it redrawn (red when the pieces go back unclipped); the one over
/// deleted and the one under it redrawn; the one over deleted and the one
/// under it the same; the one under deleted and the one over it the same
/// and partly transparent -- the last three red when a piece is clipped
/// against every image held. Named breaks: the pieces put back unclipped;
/// clipped against the images the same; clipped against the images redrawn
/// under them.
#[test]
fn render_hidden_clips_a_gone_image_only_to_the_images_redrawn_over_it() {
    let wrong: Vec<String> = [
        Overlap::UnderGoneOverRedrawn,
        Overlap::OverGoneUnderRedrawn,
        Overlap::OverGoneUnderSame,
        Overlap::UnderGoneOverSameTranslucent,
    ]
    .into_iter()
    .filter_map(gone_over_live)
    .collect();
    assert!(wrong.is_empty(), "{wrong:?}");
}

/// Whether [`written_after_an_edit`] resizes before the kept frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Resize {
    /// The terminal shrunk a row and the renderer fitted to it.
    Shrunk,
    /// Nothing between the edit and the kept frame.
    None,
}

/// The pixels [`Renderer::render_hidden`] writes into a target filled with
/// a word no frame draws, as indices into the frame, after the last frame
/// drew an opaque kitty image over the text and its frame was edited in
/// place (`a=f` with `r=1`), the terminal shrunk a row before the kept
/// frame when `resize` says and the renderer fitted to it
/// (`Renderer::fit`), so it holds no last frame. The premise: the renderer
/// took the new size.
fn written_after_an_edit(resize: Resize) -> Vec<usize> {
    const RED: [u8; 4] = [200, 40, 40, 255];
    const BLUE: [u8; 4] = [40, 40, 200, 255];
    const UNDRAWN: u32 = 0x5a5a_a5a5;
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(1 << 16));
    let mut t = Term::new(owned.storage(), &WS);
    let mut r = renderer(&faces, Px::of(0), None);
    let m = r.metrics();
    t.set_cell_size(
        ImagePx::of(u16::try_from(m.cell_w.value()).unwrap()),
        ImagePx::of(u16::try_from(m.cell_h.value()).unwrap()),
    );
    feed(
        &mut t,
        "\x1b[1 q$ ls\r\nbaud-app docs\r\n$ vim main.rs\x1b[2;1H",
    );
    feed(
        &mut t,
        &kitty("a=T,f=32,s=32,v=32,i=1,C=1,q=2", &[RED; 1024]),
    );
    feed(&mut t, "\x1b[6;1H");
    let (w, h) = r.whole_size();
    let width = usize::try_from(w.value()).expect("a width");
    let height = usize::try_from(h.value()).expect("a height");
    let mut live = vec![0u32; width * height];
    paint(&mut r, &mut t, &mut live, Damage::Full);
    assert_eq!(
        r.image_placements().len(),
        1,
        "the premise: the image drawn"
    );
    feed(
        &mut t,
        &kitty("a=f,f=32,s=32,v=32,x=0,y=0,r=1,i=1,q=2", &[BLUE; 1024]),
    );
    if resize == Resize::Shrunk {
        let _ = t.resize(&WinSize {
            ws_row: WS.ws_row - 1,
            ws_col: WS.ws_col,
        });
        r.fit(&t);
        assert_eq!(
            r.main_rows().index(),
            usize::from(WS.ws_row - 1),
            "the premise: the renderer fitted to the new size"
        );
    }
    live.fill(UNDRAWN);
    {
        let mut target = Target::new(&mut live, w, w, h).unwrap();
        r.render_hidden(&t, &mut target);
    }
    (0..live.len()).filter(|&i| live[i] != UNDRAWN).collect()
}

/// **The kept frame of a renderer that holds no last frame paints no row
/// of an image whose pixels moved** (FS-C8): [`Renderer::hide_cursor`]
/// converts nothing before a first frame or after a resize, whose rows
/// are blank, and the kept frame is then what the target holds; the rows
/// of an image whose pixels moved were marked and painted all the same,
/// from those blank rows, with the image over them. A kitty image's frame
/// edited after the last frame, the terminal shrunk a row and the renderer
/// fitted to it, then the kept frame: it writes no pixel. The control:
/// with no resize it writes some. (Master blits every image over the text
/// into the whole kept frame, so it writes the image's pixels here too.)
#[test]
fn render_hidden_after_a_resize_paints_no_row_of_an_image_whose_pixels_moved() {
    assert!(
        !written_after_an_edit(Resize::None).is_empty(),
        "the control: the kept frame paints with no resize"
    );
    let written = written_after_an_edit(Resize::Shrunk);
    assert!(
        written.is_empty(),
        "{} pixels written, the first at {:?}",
        written.len(),
        written.first()
    );
}
