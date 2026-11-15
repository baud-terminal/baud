//! **The kept frame clips a gone image's pieces to every image redrawn
//! over it, no wider, and puts each clip back right after its own piece**:
//! [`Renderer::render_hidden`] keeps an image the terminal no longer holds
//! in pieces taken from the last frame on the rows it paints, and where an
//! image drawn after the gone one in the passes' order was redrawn, the
//! piece is clipped to what the rows painted drew there. A fresh render
//! draws no gone image, so the kept frame's own rule is the comparator, on
//! every pixel of every image: where the topmost gone image covering the
//! pixel has no image redrawn over it there, the screen as the last frame
//! drew it with no cursor; everywhere else, the screen drawn whole with no
//! cursor after.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{
    BytePos, Col, Cols, ImageAlpha, ImageBudget, ImagePx, KittyId, ReadLen, Row, Rows, WinSize,
};
use baud_font::{Face, Px};
use baud_render::{Blend, Damage, ImagePlacement, Leaving, PxSize, Renderer, Stack, Target, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 6,
    ws_col: 20,
};

fn feed(t: &mut Term<'_>, bytes: &str) {
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn renderer<'f, 'a>(faces: &'f [Face<'a>]) -> Renderer<'f, 'a> {
    let mut r = Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    r.set_margins(Px::of(0));
    r.set_lights_line(None, 2.0);
    r
}

fn paint(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, px: &mut [u32], d: Damage) {
    r.fit(t);
    r.place_marks(t, Rows::ZERO, Leaving::None, Px::of(0));
    let (w, h) = r.whole_size();
    let mut target = Target::new(px, w, w, h).unwrap();
    r.render(t, &mut target, d);
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

/// A graphics command (`APC G`): the keys `ctrl`, and `px`'s RGBA pixels
/// as its payload when there are any.
fn graphics(ctrl: &str, px: &[[u8; 4]]) -> String {
    let mut v = format!("\x1b_G{ctrl}");
    if !px.is_empty() {
        v.push(';');
        v.push_str(&b64(&px.concat()));
    }
    v.push_str("\x1b\\");
    v
}

/// What becomes of one image of an arm after the last frame drew it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fate {
    /// Deleted with its data (`a=d,d=I`).
    Gone,
    /// Its frame edited in place (`a=f` with `r=1`), in other pixels.
    Redrawn,
}

/// One opaque 32 by 32 image of an arm: its id, its `z`, the cell its
/// top-left corner is placed at (row and column from zero) and its fate.
#[derive(Clone, Copy, Debug)]
struct Shown {
    id: KittyId,
    z: i32,
    row: Row,
    col: Col,
    fate: Fate,
}

/// The arms of [`kept`]: which images are drawn, where, and which go and
/// which are redrawn. Each overlap is a cell's step, so a few pixels wide
/// and tall on any cell this font gives.
#[derive(Clone, Copy, Debug)]
enum Arm {
    /// A gone image at `z=0`, one redrawn over it at `z=1`, and a second
    /// gone image over both at `z=2`: the second gone image is over the
    /// redrawn one where all three meet, so the first one's clip goes back
    /// before the second one's piece.
    PutOrder,
    /// Under the text: a gone image at `z=-2`, one redrawn over it at
    /// `z=-1` and one redrawn under it at `z=-3`: the pieces of the pass
    /// under the text are clipped to the one over it and not to the one
    /// under it.
    Under,
    /// A gone image at `z=0` and two redrawn over it, at `z=1` and `z=2`,
    /// each over a part of it the other is not: the piece is clipped to
    /// both.
    TwoRedrawn,
    /// A gone image at `z=0` and one redrawn over it at `z=1`, two cells
    /// to its right and a row down: on the rows they share, the gone
    /// image's columns left of the redrawn one stay as the last frame drew
    /// them.
    Spill,
}

impl Arm {
    fn images(self) -> Vec<Shown> {
        let at = |id: u32, z: i32, row: usize, col: usize, fate: Fate| Shown {
            id: KittyId::of(id),
            z,
            row: Row::from_index(row),
            col: Col::at_index(col),
            fate,
        };
        match self {
            Arm::PutOrder => vec![
                at(1, 0, 0, 0, Fate::Gone),
                at(2, 1, 1, 1, Fate::Redrawn),
                at(3, 2, 1, 2, Fate::Gone),
            ],
            Arm::Under => vec![
                at(1, -2, 0, 0, Fate::Gone),
                at(2, -1, 1, 1, Fate::Redrawn),
                at(3, -3, 0, 2, Fate::Redrawn),
            ],
            Arm::TwoRedrawn => vec![
                at(1, 0, 0, 0, Fate::Gone),
                at(2, 1, 0, 2, Fate::Redrawn),
                at(3, 2, 1, 0, Fate::Redrawn),
            ],
            Arm::Spill => vec![at(1, 0, 0, 0, Fate::Gone), at(2, 1, 1, 2, Fate::Redrawn)],
        }
    }

    /// The two regions each arm is about, by name: each must hold pixels
    /// the screen draws otherwise after than before, or the arm cannot
    /// fail there.
    fn claims(self) -> [&'static str; 2] {
        match self {
            Arm::PutOrder => [
                "where the first gone, the redrawn and the second gone meet",
                "where the first gone and the redrawn meet outside the second gone",
            ],
            Arm::Under => [
                "where the gone and the one over it meet",
                "where the gone and the one under it meet outside the one over it",
            ],
            Arm::TwoRedrawn => [
                "where the gone and the first redrawn meet",
                "where the gone and the second redrawn meet outside the first",
            ],
            Arm::Spill => [
                "where the gone and the redrawn meet",
                "the gone one's columns outside the redrawn one, on its rows",
            ],
        }
    }

    /// Whether a pixel is in claim `c` of this arm: `inside(k)` whether
    /// the arm's image `k` covers it, `rows(k)` whether it is on image
    /// `k`'s pixel rows.
    fn holds(self, c: usize, inside: &dyn Fn(usize) -> bool, rows: &dyn Fn(usize) -> bool) -> bool {
        match (self, c) {
            (Arm::PutOrder, 0) => inside(0) && inside(1) && inside(2),
            (Arm::PutOrder, _) => inside(0) && inside(1) && !inside(2),
            (Arm::Under, 0) | (Arm::TwoRedrawn, 0) | (Arm::Spill, 0) => inside(0) && inside(1),
            (Arm::Under, _) | (Arm::TwoRedrawn, _) => inside(0) && inside(2) && !inside(1),
            (Arm::Spill, _) => inside(0) && !inside(1) && rows(1),
        }
    }
}

/// The kept frame of `arm`'s images, the last frame having drawn them all
/// with the cursor on the last row, then each gone or redrawn as the arm
/// says, against the kept frame's own rule on every pixel of every image:
/// where the topmost gone image covering it has no image redrawn over it
/// there (later in the passes' order), the screen drawn whole with no
/// cursor before (`before`), and the screen drawn whole with no cursor
/// after (`after`) everywhere else. A description of the pixels that
/// differ, if any, with how many fall in each of the arm's claims. The
/// premises: the cursor drawn, on a row no image reaches; every image
/// placed, opaque, in its layer, in the order of its `z`; each claim holds
/// pixels drawn otherwise after; the gone images gone and the redrawn ones
/// held, their pixels moved.
fn kept(arm: Arm) -> Option<String> {
    const FIRST: [[u8; 4]; 3] = [[200, 40, 40, 255], [40, 200, 40, 255], [40, 40, 200, 255]];
    const AGAIN: [[u8; 4]; 3] = [
        [200, 200, 40, 255],
        [40, 200, 200, 255],
        [200, 40, 200, 255],
    ];
    let images = arm.images();
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(1 << 16));
    let mut t = Term::new(owned.storage(), &WS);
    let mut r = renderer(&faces);
    let m = r.metrics();
    t.set_cell_size(
        ImagePx::of(u16::try_from(m.cell_w.value()).unwrap()),
        ImagePx::of(u16::try_from(m.cell_h.value()).unwrap()),
    );
    feed(&mut t, "\x1b[1 q$ ls\r\nbaud-app docs\r\n$ vim main.rs");
    for (k, s) in images.iter().enumerate() {
        feed(
            &mut t,
            &format!("\x1b[{};{}H", s.row.index() + 1, s.col.index() + 1),
        );
        feed(
            &mut t,
            &graphics(
                &format!("a=T,f=32,s=32,v=32,i={},z={},C=1,q=2", s.id.raw(), s.z),
                &[FIRST[k]; 1024],
            ),
        );
    }
    feed(&mut t, "\x1b[6;1H");
    let (w, h) = r.whole_size();
    let width = usize::try_from(w.value()).expect("a width");
    let height = usize::try_from(h.value()).expect("a height");
    feed(&mut t, "\x1b[?25l");
    let mut fresh = renderer(&faces);
    let mut before = vec![0u32; width * height];
    paint(&mut fresh, &mut t, &mut before, Damage::Full);
    feed(&mut t, "\x1b[?25h");
    let mut live = vec![0u32; width * height];
    paint(&mut r, &mut t, &mut live, Damage::Full);
    assert_ne!(live, before, "{arm:?}: the premise: the cursor drawn");
    // Each image's place in the order the passes draw in, and its placement.
    let places = r.image_placements().to_vec();
    assert_eq!(
        places.len(),
        images.len(),
        "{arm:?}: the premise: every image placed"
    );
    let found: Vec<(usize, ImagePlacement)> = images
        .iter()
        .map(|s| {
            places
                .iter()
                .copied()
                .enumerate()
                .find(|(_, p)| p.tie == s.id)
                .unwrap_or_else(|| panic!("{arm:?}: the premise: image {} placed", s.id.raw()))
        })
        .collect();
    for (k, s) in images.iter().enumerate() {
        let p = found[k].1;
        assert!(
            p.alpha != ImageAlpha::Partial && p.layer.under_text() == (s.z < 0),
            "{arm:?}: the premise: image {} opaque and in its layer",
            s.id.raw()
        );
        for (j, o) in images.iter().enumerate() {
            assert_eq!(
                found[k].0 < found[j].0,
                s.z < o.z,
                "{arm:?}: the premise: images {} and {} drawn in the order of their z",
                s.id.raw(),
                o.id.raw()
            );
        }
    }
    // A placement's pixel columns and rows in the frame.
    let span = |p: &ImagePlacement| {
        let (x0, y0) = (
            usize::try_from(p.x).unwrap_or(0),
            usize::try_from(p.y).unwrap_or(0),
        );
        (
            x0..(x0 + usize::from(p.w)).min(width),
            y0..(y0 + usize::from(p.h)).min(height),
        )
    };
    let spans: Vec<(core::ops::Range<usize>, core::ops::Range<usize>)> =
        found.iter().map(|(_, p)| span(p)).collect();
    let ch = usize::try_from(m.cell_h.value()).expect("a cell height");
    let band = 5 * ch..6 * ch;
    assert!(
        band.end <= height && spans.iter().all(|(_, ys)| ys.end <= band.start),
        "{arm:?}: the premise: the cursor's row, {band:?}, below every image, {spans:?}"
    );
    for (k, s) in images.iter().enumerate() {
        match s.fate {
            Fate::Gone => feed(
                &mut t,
                &graphics(&format!("a=d,d=I,i={},q=2", s.id.raw()), &[]),
            ),
            Fate::Redrawn => feed(
                &mut t,
                &graphics(
                    &format!("a=f,f=32,s=32,v=32,x=0,y=0,r=1,i={},q=2", s.id.raw()),
                    &[AGAIN[k]; 1024],
                ),
            ),
        }
    }
    {
        let imgs = t.images();
        for s in &images {
            assert_eq!(
                imgs.kitty_image(s.id).is_some(),
                s.fate == Fate::Redrawn,
                "{arm:?}: the premise: image {} held exactly when redrawn",
                s.id.raw()
            );
        }
    }
    feed(&mut t, "\x1b[?25l");
    let mut now = renderer(&faces);
    let mut after = vec![0u32; width * height];
    paint(&mut now, &mut t, &mut after, Damage::Full);
    feed(&mut t, "\x1b[?25h");
    let inside = |k: usize, x: usize, y: usize| spans[k].0.contains(&x) && spans[k].1.contains(&y);
    let pixels: Vec<(usize, usize)> = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .filter(|&(x, y)| (0..images.len()).any(|k| inside(k, x, y)))
        .collect();
    let moved = |(x, y): (usize, usize)| before[y * width + x] != after[y * width + x];
    for (k, s) in images.iter().enumerate() {
        if s.fate == Fate::Redrawn {
            assert!(
                pixels
                    .iter()
                    .any(|&(x, y)| inside(k, x, y) && moved((x, y))),
                "{arm:?}: the premise: image {}'s pixels moved",
                s.id.raw()
            );
        }
    }
    let names = arm.claims();
    let claim = |c: usize, (x, y): (usize, usize)| {
        arm.holds(c, &|k: usize| inside(k, x, y), &|k: usize| {
            spans[k].1.contains(&y)
        })
    };
    for (c, name) in names.iter().enumerate() {
        assert!(
            pixels.iter().any(|&p| claim(c, p) && moved(p)),
            "{arm:?}: the premise: pixels {name}, drawn otherwise after, {spans:?}"
        );
    }
    // The kept frame's own rule.
    let order = |k: usize| found[k].0;
    let want = |x: usize, y: usize| {
        let top_gone = (0..images.len())
            .filter(|&k| images[k].fate == Fate::Gone && inside(k, x, y))
            .max_by_key(|&k| order(k));
        let redrawn_over = |g: usize| {
            (0..images.len())
                .any(|k| images[k].fate == Fate::Redrawn && order(k) > order(g) && inside(k, x, y))
        };
        match top_gone {
            Some(g) if !redrawn_over(g) => before[y * width + x],
            _ => after[y * width + x],
        }
    };
    {
        let mut target = Target::new(&mut live, w, w, h).unwrap();
        r.render_hidden(&t, &mut target);
    }
    let wrong: Vec<(usize, usize)> = pixels
        .iter()
        .copied()
        .filter(|&(x, y)| live[y * width + x] != want(x, y))
        .collect();
    (!wrong.is_empty()).then(|| {
        let mut rows: Vec<usize> = wrong.iter().map(|&(_, y)| y / ch).collect();
        rows.dedup();
        let hits: Vec<String> = names
            .iter()
            .enumerate()
            .map(|(c, name)| {
                let n = wrong.iter().filter(|&&p| claim(c, p)).count();
                format!("{n} {name}")
            })
            .collect();
        format!(
            "{arm:?}: {} pixels differ from the kept frame's rule, the first at {:?}, on cell rows {rows:?}: {hits:?}",
            wrong.len(),
            wrong[0]
        )
    })
}

/// **The kept frame clips a gone image's pieces to every image redrawn
/// over it, no wider, and puts each clip back right after its own
/// piece**: a gone image's pieces are taken from the last frame, so where
/// an image drawn over it was redrawn they hold that image's last pixels,
/// and the clip, taken from the rows painted, puts its new ones back over
/// them. Four arms, each run whatever the others do, the failures
/// collected: a second gone image over the redrawn one stays over it where
/// all three meet, so each clip goes back before the next piece; under
/// the text, the pieces are clipped to the image redrawn over the gone one
/// and not to the one redrawn under it; a piece is clipped to each of two
/// images redrawn over it; and outside the redrawn image's columns, on
/// its rows, the gone image stays as the last frame drew it.
#[test]
fn render_hidden_clips_a_gone_image_to_every_image_redrawn_over_it_and_no_wider() {
    let wrong: Vec<String> = [Arm::PutOrder, Arm::Under, Arm::TwoRedrawn, Arm::Spill]
        .into_iter()
        .filter_map(kept)
        .collect();
    assert!(wrong.is_empty(), "{wrong:?}");
}
