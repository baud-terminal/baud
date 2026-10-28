//! What a kitty placement draws, on the CPU compositor, against the
//! governing text (`[[source: kitty-graphics]]`, "Controlling displayed
//! image layout"):
//!
//! * the source rectangle -- "The displayed area is the intersection of the
//!   specified rectangle with the source image rectangle";
//! * `c` and `r` -- "The image will be scaled (enlarged/shrunk) as needed to
//!   fit the specified area", "If only one of either r or c is specified,
//!   the other one is computed based on the source image aspect ratio", and
//!   "When both are specified the image is letterboxed/pillarboxed";
//! * `X` and `Y` -- "extra X=3 and Y=4 pixel offsets to display from a
//!   different origin within the cell", "not added to the number of
//!   rows/columns";
//! * `z` -- "Negative z-index values mean that the images will be drawn
//!   under the text", and "If two images with the same z-index overlap then
//!   the image with the lower id is considered to have the lower z-index".
//!
//! Every expected pixel is computed here from the text's rules, never by
//! the renderer's own geometry: the box, the aspect rounded to the nearest
//! pixel, centred in the box when both counts are given (Jev, CENTRED,
//! decisions.jsonl lines 626-627), and each drawn pixel the source texel
//! under its centre (nearest).

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, ImageBudget, ImagePx, ReadLen};
use baud_domain::{Col, Row};
use baud_font::Face;
use baud_render::{Damage, Highlight, Region, Theme};
use support::{Canvas, WS, font, renderer};

/// The test image: 6 by 4, every pixel opaque and distinct.
const IW: u32 = 6;
const IH: u32 = 4;

fn texel(x: u32, y: u32) -> u32 {
    0xFF00_0000 | ((37 * x + 20) << 16) | ((53 * y + 10) << 8) | 200
}

/// A second image, 6 by 4, in other colours: which of two is on top.
fn other(x: u32, y: u32) -> u32 {
    0xFF00_0000 | (200 << 16) | ((29 * x + 5) << 8) | (41 * y + 30)
}

fn rgba(px: impl Fn(u32, u32) -> u32) -> Vec<u8> {
    let mut v = Vec::new();
    for y in 0..IH {
        for x in 0..IW {
            let p = px(x, y);
            let [_, r, g, b] = p.to_be_bytes();
            v.extend_from_slice(&[r, g, b, 0xFF]);
        }
    }
    v
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for q in data.chunks(3) {
        let mut b = [0u8; 3];
        b[..q.len()].copy_from_slice(q);
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        for k in 0..4 {
            if k <= q.len() {
                out.push(B64[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]);
            } else {
                out.push(b'=');
            }
        }
    }
    out
}

fn apc(ctrl: &str, payload: &[u8]) -> Vec<u8> {
    let mut v = b"\x1b_G".to_vec();
    v.extend_from_slice(ctrl.as_bytes());
    if !payload.is_empty() {
        v.push(b';');
        v.extend_from_slice(payload);
    }
    v.extend_from_slice(b"\x1b\\");
    v
}

fn feed(t: &mut Term, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// Transmit image `id` with `px`'s pixels, placing nothing.
fn transmit(t: &mut Term, id: u32, px: impl Fn(u32, u32) -> u32) {
    feed(
        t,
        &apc(
            &format!("a=t,f=32,s={IW},v={IH},i={id},q=2"),
            &b64(&rgba(px)),
        ),
    );
}

/// The anchor: row 2, column 4, zero-based.
const AT: &[u8] = b"\x1b[3;5H";
const ROW: u32 = 2;
const COL: u32 = 4;

/// The value of key `name` in a control string, zero when absent.
fn key(ctrl: &str, name: char) -> u32 {
    ctrl.split(',')
        .find_map(|kv| kv.strip_prefix(name)?.strip_prefix('='))
        .map_or(0, |v| v.parse().unwrap())
}

/// Where the text says the placement draws, from the anchor cell's
/// top-left (`dest`: x, y, width, height), and the source rectangle it
/// maps (`src`, the same four).
#[derive(Debug, Clone, Copy)]
struct Expect {
    dest: [u32; 4],
    src: [u32; 4],
}

/// `a / b` to the nearest integer, a half up.
fn nearest(a: u64, b: u64) -> u32 {
    u32::try_from((a + b / 2) / b).unwrap()
}

fn expect(ctrl: &str, cw: u32, ch: u32) -> Expect {
    let k = |n| key(ctrl, n);
    let sx = k('x').min(IW);
    let sy = k('y').min(IH);
    let sw = if k('w') == 0 {
        IW - sx
    } else {
        k('w').min(IW - sx)
    };
    let sh = if k('h') == 0 {
        IH - sy
    } else {
        k('h').min(IH - sy)
    };
    let (sw64, sh64) = (u64::from(sw), u64::from(sh));
    let (ox, oy) = (k('X'), k('Y'));
    let (mut dx, mut dy) = (ox, oy);
    let (dw, dh) = match (k('c'), k('r')) {
        (0, 0) => (sw, sh),
        (c, 0) => {
            let dw = c * cw - ox;
            (dw, nearest(u64::from(dw) * sh64, sw64))
        }
        (0, r) => {
            let dh = r * ch - oy;
            (nearest(u64::from(dh) * sw64, sh64), dh)
        }
        (c, r) => {
            let (bw, bh) = (c * cw - ox, r * ch - oy);
            if u64::from(bw) * sh64 < u64::from(bh) * sw64 {
                let dh = nearest(u64::from(bw) * sh64, sw64);
                dy += (bh - dh) / 2;
                (bw, dh)
            } else {
                let dw = nearest(u64::from(bh) * sw64, sh64);
                dx += (bw - dw) / 2;
                (dw, bh)
            }
        }
    };
    Expect {
        dest: [dx, dy, dw, dh],
        src: [sx, sy, sw, sh],
    }
}

/// The source texel under drawn pixel `(i, j)`'s centre.
fn source(e: Expect, i: u32, j: u32) -> (u32, u32) {
    let [sx, sy, sw, sh] = e.src;
    let [_, _, dw, dh] = e.dest;
    // The centre `d + 1/2` of `n` drawn pixels over `s` source ones.
    let f = |d: u32, off: u32, s: u32, n: u32| off + (2 * d + 1) * s / (2 * n);
    (f(i, sx, sw, dw), f(j, sy, sh, dh))
}

/// A frame and the cell size it was drawn at, width then height.
struct Frame {
    c: Canvas,
    cell: [u32; 2],
}

impl Frame {
    fn cw(&self) -> u32 {
        self.cell[0]
    }

    fn ch(&self) -> u32 {
        self.cell[1]
    }

    fn at(&self, x: u32, y: u32) -> u32 {
        let (x, y) = (usize::try_from(x).unwrap(), usize::try_from(y).unwrap());
        self.c.pixels[y * self.c.width + x]
    }
}

/// A terminal with image memory and the renderer's cell size, cursor
/// hidden, after `setup`; then one full frame.
fn draw(setup: impl FnOnce(&mut Term)) -> Frame {
    draw_lit(setup, None)
}

/// [`draw`], with a selection drawn.
fn draw_lit(setup: impl FnOnce(&mut Term), lit: Option<Highlight>) -> Frame {
    draw_themed(setup, lit, Theme::dark())
}

/// [`draw_lit`] under `theme`.
fn draw_themed(setup: impl FnOnce(&mut Term), lit: Option<Highlight>, theme: Theme) -> Frame {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    r.set_theme(theme);
    r.set_highlight(lit);
    let m = r.metrics();
    let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(
        ImagePx::of(u16::try_from(cw).unwrap()),
        ImagePx::of(u16::try_from(ch).unwrap()),
    );
    feed(&mut t, b"\x1b[?25l");
    setup(&mut t);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    Frame { c, cell: [cw, ch] }
}

/// One placement of the test image at the anchor, every pixel of its
/// neighbourhood held to the text: inside the drawn rectangle the source
/// texel under the pixel's centre, outside it the background. Returns how
/// many pixels miss, after asserting the rectangle is not empty.
fn misses(ctrl: &str) -> usize {
    let f = draw(|t| {
        transmit(t, 1, texel);
        feed(t, AT);
        feed(t, &apc(&format!("a=p,i=1,q=2,{ctrl}"), b""));
    });
    let e = expect(ctrl, f.cw(), f.ch());
    let [dx, dy, dw, dh] = e.dest;
    assert!(dw > 0 && dh > 0, "the premise: {e:?} draws");
    let bg = Theme::dark().bg.0;
    let (ax, ay) = (COL * f.cw(), ROW * f.ch());
    let mut wrong = 0;
    for y in ay..ay + dy + dh + f.ch() {
        for x in ax..ax + dx + dw + f.cw() {
            let inside =
                (ax + dx..ax + dx + dw).contains(&x) && (ay + dy..ay + dy + dh).contains(&y);
            let want = match inside {
                true => {
                    let (sx, sy) = source(e, x - ax - dx, y - ay - dy);
                    texel(sx, sy)
                }
                false => bg,
            };
            if f.at(x, y) != want {
                wrong += 1;
            }
        }
    }
    wrong
}

#[test]
fn the_source_rectangle_is_what_is_drawn() {
    // A rectangle inside the image, and one past its right edge, which
    // draws "the intersection of the specified rectangle with the source
    // image rectangle".
    for k in ["x=2,y=1,w=3,h=2", "x=4,w=10"] {
        assert_eq!(misses(k), 0, "the source rectangle {k}");
    }
}

#[test]
fn c_alone_scales_to_the_columns_and_keeps_the_aspect() {
    for k in ["c=1", "c=3", "c=7"] {
        assert_eq!(misses(k), 0, "{k}");
    }
}

#[test]
fn r_alone_scales_to_the_rows_and_keeps_the_aspect() {
    for k in ["r=1", "r=2", "r=4"] {
        assert_eq!(misses(k), 0, "{k}");
    }
}

#[test]
fn c_and_r_letterbox_or_pillarbox_the_image_in_the_box() {
    // A wide box pillarboxes the 3:2 image, a tall one letterboxes it.
    for k in ["c=4,r=1", "c=1,r=3", "c=6,r=2"] {
        assert_eq!(misses(k), 0, "{k}");
    }
}

#[test]
fn x_and_y_offset_the_image_inside_the_first_cell() {
    for k in ["X=3,Y=5", "X=3,Y=5,c=2", "X=1,Y=2,x=1,y=1,w=4,h=2,c=5,r=1"] {
        assert_eq!(misses(k), 0, "{k}");
    }
}

/// Upper half blocks over the anchor's row, and the image covering four of
/// its cells at `z`. A block element is drawn as a solid, so its pixels are
/// the text's colour exactly and the half below it the background: what
/// the image under it must show through is known to the pixel.
fn under_text(z: i32) -> (Frame, Frame) {
    let text = |t: &mut Term| feed(t, "\x1b[3;5H\u{2580}\u{2580}\u{2580}\u{2580}".as_bytes());
    let plain = draw(text);
    let imaged = draw(|t| {
        text(t);
        transmit(t, 1, texel);
        feed(t, AT);
        feed(t, &apc(&format!("a=p,i=1,q=2,c=4,r=1,z={z}"), b""));
    });
    (plain, imaged)
}

/// Of the drawn rectangle of an image placed `c=4,r=1` over the blocks,
/// how many pixels are the text's and how many the image's, and how many
/// are neither of what they should be: under the text, the text's pixel
/// where it drew one and the texel elsewhere; over it, the texel.
fn layered(z: i32) -> (usize, usize, usize) {
    let bg = Theme::dark().bg.0;
    let (plain, imaged) = under_text(z);
    let e = expect("c=4,r=1", plain.cw(), plain.ch());
    let [dx, dy, dw, dh] = e.dest;
    let (ax, ay) = (COL * plain.cw() + dx, ROW * plain.ch() + dy);
    let (mut ink, mut bare, mut wrong) = (0, 0, 0);
    for j in 0..dh {
        for i in 0..dw {
            let (x, y) = (ax + i, ay + j);
            let (sx, sy) = source(e, i, j);
            let under = plain.at(x, y);
            let want = match (z < 0, under == bg) {
                (true, false) => {
                    ink += 1;
                    under
                }
                (true, true) => {
                    bare += 1;
                    texel(sx, sy)
                }
                (false, _) => texel(sx, sy),
            };
            wrong += usize::from(imaged.at(x, y) != want);
        }
    }
    (ink, bare, wrong)
}

#[test]
fn a_negative_z_draws_under_the_text() {
    let (ink, bare, wrong) = layered(-1);
    assert!(ink > 0 && bare > 0, "the premise: ink {ink}, bare {bare}");
    assert_eq!(wrong, 0, "the text over the image, the image between");
    // The control: at z = 0 the same image covers the text.
    assert_eq!(layered(0).2, 0, "at z = 0 the image covers it");
}

/// Two images placed on one cell in the order given, each `(id, z,
/// pixels)`; which one shows at the anchor's first pixel.
fn on_top(first: (u32, i32), second: (u32, i32)) -> u32 {
    let f = draw(|t| {
        transmit(t, first.0, texel);
        transmit(t, second.0, other);
        for (id, z) in [first, second] {
            feed(t, AT);
            feed(t, &apc(&format!("a=p,i={id},q=2,z={z}"), b""));
        }
    });
    f.at(COL * f.cw(), ROW * f.ch())
}

#[test]
fn a_higher_z_draws_over_a_lower_one_whatever_the_order() {
    // `texel` placed first at z = 2, `other` second at z = 1, its id the
    // higher: the order is `z`'s, not the id's or the placement's.
    assert_eq!(on_top((1, 2), (2, 1)), texel(0, 0), "z = 2 over z = 1");
    // The control: the same order at the same z draws the second on top.
    assert_eq!(on_top((1, 1), (2, 1)), other(0, 0), "the later, higher id");
    // And under the text, ordered among themselves.
    assert_eq!(on_top((1, -2), (2, -5)), texel(0, 0), "z = -2 over -5");
}

#[test]
fn at_one_z_the_lower_id_draws_under() {
    // `texel` is id 7, placed first; `other` id 3, placed second.
    assert_eq!(on_top((7, 0), (3, 0)), texel(0, 0), "id 7 over id 3");
}

#[test]
fn an_image_under_the_text_appears_and_goes_on_incremental_frames() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let m = r.metrics();
    let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(
        ImagePx::of(u16::try_from(cw).unwrap()),
        ImagePx::of(u16::try_from(ch).unwrap()),
    );
    feed(
        &mut t,
        "\x1b[?25l\x1b[3;5H\u{2580}\u{2580}\u{2580}\u{2580}".as_bytes(),
    );
    transmit(&mut t, 1, texel);
    // The image's drawn rectangle, every pixel of it, row-major.
    let e = expect("c=4,r=1", cw, ch);
    let [dx, dy, dw, dh] = e.dest;
    let rect = |c: &Canvas| {
        let at = |n: u32| usize::try_from(n).unwrap();
        (at(ROW * ch + dy)..at(ROW * ch + dy + dh))
            .flat_map(|y| {
                let from = y * c.width + at(COL * cw + dx);
                c.pixels[from..from + at(dw)].to_vec()
            })
            .collect::<Vec<u32>>()
    };
    let mut frame = |t: &mut Term, damage| {
        r.render(t, &mut c.target(), damage);
        rect(&c)
    };
    let bg = Theme::dark().bg.0;
    let plain = frame(&mut t, Damage::Full);
    feed(&mut t, AT);
    feed(&mut t, &apc("a=p,i=1,p=1,q=2,c=4,r=1,z=-1", b""));
    let placed = frame(&mut t, Damage::Incremental);
    let want: Vec<u32> = (0..dh)
        .flat_map(|j| (0..dw).map(move |i| (i, j)))
        .zip(&plain)
        .map(|((i, j), &p)| match p == bg {
            true => {
                let (sx, sy) = source(e, i, j);
                texel(sx, sy)
            }
            false => p,
        })
        .collect();
    assert!(
        plain.iter().any(|&p| p != bg),
        "the premise: text in the rect"
    );
    assert_eq!(
        placed, want,
        "placed under the text on an incremental frame"
    );
    feed(&mut t, &apc("a=d,d=i,i=1,p=1,q=2", b""));
    assert_eq!(
        frame(&mut t, Damage::Incremental),
        plain,
        "deleted, gone without a full frame"
    );
}

/// "When scrolling them would cause them to extend outside the page area,
/// they must be clipped" (`[[source: kitty-graphics]]`, "Interaction with
/// other terminal actions"): an image two rows tall at the top margin,
/// scrolled up a row, draws its second row at the margin and nothing above
/// it.
#[test]
fn an_image_scrolled_past_a_margin_is_clipped_at_it() {
    let f = draw(|t| {
        feed(t, b"\x1b[3;10r");
        transmit(t, 1, texel);
        feed(t, AT);
        feed(t, &apc("a=p,i=1,q=2,C=1,r=2", b""));
        feed(t, b"\x1b[10;1H\n");
    });
    let e = expect("r=2", f.cw(), f.ch());
    let [_, _, dw, dh] = e.dest;
    assert_eq!(dh, 2 * f.ch(), "the premise: two rows tall");
    let bg = Theme::dark().bg.0;
    let ax = COL * f.cw();
    let mut wrong = 0;
    // The row above the margin: background.
    for y in (ROW - 1) * f.ch()..ROW * f.ch() {
        for x in ax..ax + dw {
            wrong += usize::from(f.at(x, y) != bg);
        }
    }
    // The margin's row: the image's second row.
    for k in 0..f.ch() {
        for i in 0..dw {
            let (sx, sy) = source(e, i, f.ch() + k);
            wrong += usize::from(f.at(ax + i, ROW * f.ch() + k) != texel(sx, sy));
        }
    }
    // The row below: the image is gone from it.
    for y in (ROW + 1) * f.ch()..(ROW + 2) * f.ch() {
        for x in ax..ax + dw {
            wrong += usize::from(f.at(x, y) != bg);
        }
    }
    assert_eq!(wrong, 0, "clipped at the margin");
}

/// The boundary the text draws: "Negative z-index values below INT32_MIN/2
/// (-1,073,741,824) will be drawn under cells with non-default background
/// colors" (`[[source: kitty-graphics]]`, "Controlling displayed image
/// layout"). Below is strictly less.
const DEEP: i32 = -1_073_741_825;
const EDGE: i32 = -1_073_741_824;

/// Four cells at the anchor -- a red-backed upper half block, a red-backed
/// space, a default-backed block and a default-backed space -- under an
/// image placed `c=4,r=1` at `z`.
const SCENE: &str = "\x1b[3;5H\x1b[41m\u{2580} \x1b[0m\u{2580} ";

/// The scene's frame without the image and with it at `z`, after `more`.
fn scene(z: i32, more: &str) -> (Frame, Frame) {
    let text = |t: &mut Term| {
        feed(t, SCENE.as_bytes());
        feed(t, more.as_bytes());
    };
    let plain = draw(text);
    let imaged = draw(|t| {
        text(t);
        transmit(t, 1, texel);
        feed(t, AT);
        feed(t, &apc(&format!("a=p,i=1,q=2,C=1,c=4,r=1,z={z}"), b""));
    });
    (plain, imaged)
}

/// What a pixel of the image's drawn rectangle should be, held to the
/// text: the plain frame's pixel where the cell drew ink -- anything but
/// its own background, read at its bottom row, which the upper half block
/// leaves bare -- and where the image is under a non-default background
/// (`deep`, and the cell one of the red two); the texel elsewhere. Returns
/// how many pixels miss, and how many of each kind there were: red bare,
/// default bare, ink.
fn deep_misses(plain: &Frame, imaged: &Frame, deep: bool) -> (usize, [usize; 3]) {
    let e = expect("c=4,r=1", plain.cw(), plain.ch());
    let [dx, dy, dw, dh] = e.dest;
    let (ax, ay) = (COL * plain.cw() + dx, ROW * plain.ch() + dy);
    let (mut wrong, mut kinds) = (0, [0; 3]);
    for j in 0..dh {
        for i in 0..dw {
            let (x, y) = (ax + i, ay + j);
            let cell = (x - COL * plain.cw()) / plain.cw();
            let own = plain.at(x, ROW * plain.ch() + plain.ch() - 1);
            let under = plain.at(x, y);
            let red = cell < 2;
            let want = if under != own {
                kinds[2] += 1;
                under
            } else if red {
                kinds[0] += 1;
                match deep {
                    true => under,
                    false => {
                        let (sx, sy) = source(e, i, j);
                        texel(sx, sy)
                    }
                }
            } else {
                kinds[1] += 1;
                let (sx, sy) = source(e, i, j);
                texel(sx, sy)
            };
            wrong += usize::from(imaged.at(x, y) != want);
        }
    }
    (wrong, kinds)
}

#[test]
fn a_z_below_half_int32_min_draws_under_non_default_backgrounds() {
    let (plain, imaged) = scene(DEEP, "");
    let bg = Theme::dark().bg.0;
    let red = plain.at(COL * plain.cw() + 1, ROW * plain.ch() + plain.ch() - 1);
    assert_ne!(red, bg, "the premise: the red cells are not the default");
    let (wrong, kinds) = deep_misses(&plain, &imaged, true);
    assert!(
        kinds.iter().all(|&k| k > 0),
        "the premise: red bare, default bare and ink {kinds:?}"
    );
    assert_eq!(wrong, 0, "under the red backgrounds, over the default");
}

#[test]
fn a_z_of_half_int32_min_exactly_stays_over_every_background() {
    let (plain, imaged) = scene(EDGE, "");
    let (wrong, kinds) = deep_misses(&plain, &imaged, false);
    assert!(kinds.iter().all(|&k| k > 0), "the premise {kinds:?}");
    assert_eq!(wrong, 0, "-1073741824 is not below it: over the red too");
}

#[test]
fn below_half_int32_min_the_order_is_still_z_then_the_lower_id() {
    // Within the layer: at one `z`, the lower id under.
    assert_eq!(on_top((7, DEEP), (3, DEEP)), texel(0, 0), "id 7 over id 3");
    assert_eq!(on_top((3, DEEP), (7, DEEP)), other(0, 0), "id 7 over id 3");
    // Across the boundary, either order: -1 over the deep one.
    assert_eq!(on_top((1, -1), (2, DEEP)), texel(0, 0), "z = -1 over");
    assert_eq!(on_top((1, DEEP), (2, -1)), other(0, 0), "z = -1 over");
    assert_eq!(on_top((1, EDGE), (2, DEEP)), texel(0, 0), "the edge over");
}

/// The reading of two forks the text does not settle, Jev's NON_DEFAULT
/// (1.00 and 1.00): a block cursor's cell and a selected cell draw a
/// background that is not the default one, so a deep image goes under them,
/// as kitty's `compute_background` makes them "special"
/// (`[[source: kitty-background-slang]]`).
#[test]
fn a_deep_image_goes_under_the_block_cursor_and_the_selection() {
    // The cursor on the default-backed block, the third cell; the
    // selection over the default-backed space, the fourth.
    let lit = Some(Highlight::new(
        (Row::from_index(2), Col::at_index(7)),
        (Row::from_index(2), Col::at_index(7)),
        Region::Stream,
    ));
    let more = "\x1b[2 q\x1b[3;7H\x1b[?25h";
    let text = |t: &mut Term| {
        feed(t, SCENE.as_bytes());
        feed(t, more.as_bytes());
    };
    let plain = draw_lit(text, lit);
    let imaged = draw_lit(
        |t| {
            text(t);
            transmit(t, 1, texel);
            feed(t, b"\x1b7");
            feed(t, AT);
            feed(t, &apc(&format!("a=p,i=1,q=2,C=1,c=4,r=1,z={DEEP}"), b""));
            feed(t, b"\x1b8");
        },
        lit,
    );
    let e = expect("c=4,r=1", plain.cw(), plain.ch());
    let [dx, dy, dw, dh] = e.dest;
    let (ax, ay) = (COL * plain.cw() + dx, ROW * plain.ch() + dy);
    let (mut wrong, mut seen) = (0, 0);
    for j in 0..dh {
        for i in 0..dw {
            let (x, y) = (ax + i, ay + j);
            if (x - COL * plain.cw()) / plain.cw() >= 2 {
                seen += 1;
                wrong += usize::from(imaged.at(x, y) != plain.at(x, y));
            }
        }
    }
    assert!(seen > 0, "the premise: the image reaches the lit cells");
    assert_eq!(wrong, 0, "the cursor and the selection over the image");
}

#[test]
fn a_deep_image_appears_goes_and_follows_a_background_on_incremental_frames() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let m = r.metrics();
    let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(
        ImagePx::of(u16::try_from(cw).unwrap()),
        ImagePx::of(u16::try_from(ch).unwrap()),
    );
    feed(&mut t, b"\x1b[?25l");
    feed(&mut t, SCENE.as_bytes());
    transmit(&mut t, 1, texel);
    let mut frame = |t: &mut Term, damage| {
        r.render(t, &mut c.target(), damage);
        Frame {
            c: Canvas {
                pixels: c.pixels.clone(),
                width: c.width,
                height: c.height,
            },
            cell: [cw, ch],
        }
    };
    let plain = frame(&mut t, Damage::Full);
    feed(&mut t, AT);
    feed(
        &mut t,
        &apc(&format!("a=p,i=1,p=1,q=2,C=1,c=4,r=1,z={DEEP}"), b""),
    );
    let placed = frame(&mut t, Damage::Incremental);
    let (wrong, kinds) = deep_misses(&plain, &placed, true);
    assert!(kinds.iter().all(|&k| k > 0), "the premise {kinds:?}");
    assert_eq!(wrong, 0, "placed under the red on an incremental frame");
    // The red space becomes a default one: the image shows through it.
    feed(&mut t, b"\x1b[3;6H\x1b[0m ");
    let repainted = frame(&mut t, Damage::Incremental);
    let e = expect("c=4,r=1", cw, ch);
    let [dx, dy, dw, dh] = e.dest;
    let x0 = (COL + 1) * cw;
    let (mut wrong, mut seen) = (0, 0);
    for j in 0..dh {
        for i in 0..dw {
            let x = COL * cw + dx + i;
            if (x0..x0 + cw).contains(&x) {
                let (sx, sy) = source(e, i, j);
                seen += 1;
                wrong += usize::from(repainted.at(x, ROW * ch + dy + j) != texel(sx, sy));
            }
        }
    }
    assert!(seen > 0, "the premise: the image reaches the second cell");
    assert_eq!(wrong, 0, "a background gone default shows the image");
    feed(&mut t, &apc("a=d,d=i,i=1,p=1,q=2", b""));
    feed(&mut t, b"\x1b[3;6H\x1b[41m \x1b[0m");
    let gone = frame(&mut t, Damage::Incremental);
    assert!(
        gone.c.pixels == plain.c.pixels,
        "deleted, gone without a full frame"
    );
}

/// Of four cells at the anchor written by `text`, under a deep image, how
/// many pixels of the image's drawn rectangle are the frame without it,
/// and how many in all.
fn deep_hidden(text: &str) -> (usize, usize) {
    let write = |t: &mut Term| feed(t, text.as_bytes());
    let plain = draw(write);
    let imaged = draw(|t| {
        write(t);
        transmit(t, 1, texel);
        feed(t, AT);
        feed(t, &apc(&format!("a=p,i=1,q=2,C=1,c=4,r=1,z={DEEP}"), b""));
    });
    let e = expect("c=4,r=1", plain.cw(), plain.ch());
    let [dx, dy, dw, dh] = e.dest;
    let (ax, ay) = (COL * plain.cw() + dx, ROW * plain.ch() + dy);
    let mut hidden = 0;
    for j in 0..dh {
        for i in 0..dw {
            hidden += usize::from(imaged.at(ax + i, ay + j) == plain.at(ax + i, ay + j));
        }
    }
    (hidden, usize::try_from(dw * dh).unwrap())
}

/// The readings of the forks the text leaves to "non-default background
/// colors": a cell is default when the colour its background is drawn in is
/// the default background, so `SGR 7` on default colours and every cell
/// under `?5` are not, as kitty compares the drawn colour
/// (`[[source: kitty-background-slang]]`; Jev's DRAWN_COLOUR, 1.00 and
/// 0.98); and a default-word cell is drawn in the default's colour
/// whatever `OSC 11` makes it, so it stays default.
#[test]
fn the_default_background_is_the_drawn_one_and_survives_osc_11() {
    let (hidden, all) = deep_hidden("\x1b[3;5H\x1b[7m    \x1b[0m");
    assert_eq!(hidden, all, "SGR 7: the background is the foreground's");
    let (hidden, all) = deep_hidden("\x1b[?5h");
    assert_eq!(hidden, all, "?5: every background is the foreground's");
    let (hidden, all) = deep_hidden("\x1b[?5h\x1b[3;5H\x1b[7m    \x1b[0m");
    assert_eq!(
        hidden, 0,
        "?5 and SGR 7 together: the default again, of {all}"
    );
    let (hidden, all) = deep_hidden("\x1b]11;rgb:30/00/30\x1b\\");
    assert_eq!(hidden, 0, "OSC 11: still the default, of {all}");
    let (hidden, all) = deep_hidden("\x1b[3;5H\x1b[48;5;1m    \x1b[0m");
    assert_eq!(hidden, all, "a palette background is not the default");
}

/// What a pixel of a cell under a deep image is expected to be, the
/// cell bare (a space): the image's texel, the frame without the image,
/// or the cell's background, straight, source-over the texel.
#[derive(Clone, Copy, Debug)]
enum Deep {
    Texel,
    Plain,
    Over(u32),
}

/// `bg`, straight with its alpha, source-over the opaque texel `t`, one
/// channel at a time: `co = Cs as + Cb (1 - as)` at `ab = 1`, rounded to
/// the nearest byte (`[[source: w3c-compositing-1]]` 5.1).
fn over_texel(bg: u32, t: u32) -> u32 {
    let a = bg >> 24;
    let ch = |shift: u32| {
        let (s, d) = ((bg >> shift) & 0xFF, (t >> shift) & 0xFF);
        ((s * a + d * (255 - a) + 127) / 255) << shift
    };
    0xFF00_0000 | ch(16) | ch(8) | ch(0)
}

/// A word as the presenter stores it, premultiplied: what two pixels
/// that differ only in a straight byte the alpha hides agree on.
fn premul(p: u32) -> u32 {
    baud_domain::premultiply(p)
}

/// Four bare cells at the anchor written by `text` under `theme`, under
/// a deep image of `px`'s pixels placed `c=4,r=1`: how many pixels of its
/// drawn rectangle are not what `want` says of their cell, compared
/// premultiplied, and how many there were.
fn deep_cells(text: &str, theme: &Theme, alpha: u8, want: [Deep; 4]) -> (usize, usize) {
    let write = |t: &mut Term| feed(t, text.as_bytes());
    let plain = draw_themed(write, None, theme.clone());
    let imaged = draw_themed(
        |t| {
            write(t);
            transmit_alpha(t, 1, alpha);
            feed(t, AT);
            feed(t, &apc(&format!("a=p,i=1,q=2,C=1,c=4,r=1,z={DEEP}"), b""));
        },
        None,
        theme.clone(),
    );
    let e = expect("c=4,r=1", plain.cw(), plain.ch());
    let [dx, dy, dw, dh] = e.dest;
    let (ax, ay) = (COL * plain.cw() + dx, ROW * plain.ch() + dy);
    let mut wrong = 0;
    for j in 0..dh {
        for i in 0..dw {
            let (x, y) = (ax + i, ay + j);
            let cell = usize::try_from((x - COL * plain.cw()) / plain.cw()).unwrap();
            let (sx, sy) = source(e, i, j);
            let w = match want[cell] {
                Deep::Texel => texel(sx, sy),
                Deep::Plain => plain.at(x, y),
                Deep::Over(bg) => over_texel(bg, texel(sx, sy)),
            };
            wrong += usize::from(premul(imaged.at(x, y)) != premul(w));
        }
    }
    (wrong, usize::try_from(dw * dh).unwrap())
}

/// Transmit image `id`, the test image's colours at `alpha`.
fn transmit_alpha(t: &mut Term, id: u32, alpha: u8) {
    let mut v = rgba(texel);
    for p in v.chunks_mut(4) {
        p[3] = alpha;
    }
    feed(
        t,
        &apc(&format!("a=t,f=32,s={IW},v={IH},i={id},q=2"), &b64(&v)),
    );
}

const DEFAULT: [Deep; 4] = [Deep::Texel; 4];
const RED_SET: [Deep; 4] = [Deep::Plain, Deep::Plain, Deep::Texel, Deep::Texel];

/// **The owner's pick, "Compare the colour" (2026-10-16)**: a cell is
/// default when the colour its background is drawn in equals the default
/// background's colour, as kitty's `cell_has_default_bg` compares it
/// (`[[source: kitty-background-slang]]`), and not when it carries the
/// default colour word. So an explicit colour, 24-bit or a palette entry,
/// whose value is the default's shows a deep image; and under one exchange
/// the drawn background is the foreground's colour, so `SGR 7` or `?5`
/// over a foreground of the default's value is default too.
#[test]
fn an_explicit_background_of_the_default_colour_shows_a_deep_image() {
    let dark = Theme::dark();
    assert_eq!(dark.bg.0 & 0xFF_FFFF, 0x10_1010, "the premise: the default");
    for (text, what) in [
        ("\x1b[3;5H\x1b[48;2;16;16;16m    \x1b[0m", "24-bit"),
        (
            "\x1b]4;1;rgb:10/10/10\x1b\\\x1b[3;5H\x1b[41m    \x1b[0m",
            "a palette entry OSC 4 set to it",
        ),
        (
            "\x1b[3;5H\x1b[38;2;16;16;16;7m    \x1b[0m",
            "SGR 7 over a foreground of it",
        ),
        (
            "\x1b[?5h\x1b[3;5H\x1b[38;2;16;16;16m    \x1b[0m",
            "?5 over a foreground of it",
        ),
    ] {
        let (wrong, all) = deep_cells(text, &dark, 0xFF, DEFAULT);
        assert_eq!(wrong, 0, "{what}: the image shows, of {all}");
    }
    // The control: a colour one step off the default's is not it.
    let (wrong, all) = deep_cells(
        "\x1b[3;5H\x1b[48;2;16;16;17m  \x1b[0m  ",
        &dark,
        0xFF,
        RED_SET,
    );
    assert_eq!(wrong, 0, "one step off: under the cell, of {all}");
}

/// After `OSC 11` the comparison moves with the default: a cell whose
/// explicit colour is the new default shows a deep image, a default-word
/// cell, drawn in the new default, shows it, and a cell whose explicit
/// colour is the old default no longer does.
#[test]
fn after_osc_11_the_comparison_moves_with_the_default() {
    let dark = Theme::dark();
    let osc = "\x1b]11;rgb:30/00/30\x1b\\";
    let (wrong, all) = deep_cells(
        &format!("{osc}\x1b[3;5H\x1b[48;2;48;0;48m  \x1b[0m  "),
        &dark,
        0xFF,
        DEFAULT,
    );
    assert_eq!(wrong, 0, "the new default's value and the word, of {all}");
    let (wrong, all) = deep_cells(
        &format!("{osc}\x1b[3;5H\x1b[48;2;16;16;16m  \x1b[0m  "),
        &dark,
        0xFF,
        RED_SET,
    );
    assert_eq!(wrong, 0, "the old default's value is set, of {all}");
}

/// The same on incremental frames: `OSC 11` moves no cell, and the cells
/// whose explicit colour it lands on show the image on the next frame,
/// and stop when `OSC 111` puts the default back.
#[test]
fn osc_11_moves_a_deep_image_under_explicit_cells_on_incremental_frames() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let m = r.metrics();
    let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(
        ImagePx::of(u16::try_from(cw).unwrap()),
        ImagePx::of(u16::try_from(ch).unwrap()),
    );
    feed(&mut t, b"\x1b[?25l\x1b[3;5H\x1b[48;2;48;0;48m    \x1b[0m");
    transmit(&mut t, 1, texel);
    feed(&mut t, AT);
    feed(
        &mut t,
        &apc(&format!("a=p,i=1,q=2,C=1,c=4,r=1,z={DEEP}"), b""),
    );
    let e = expect("c=4,r=1", cw, ch);
    let [dx, dy, dw, dh] = e.dest;
    let mut shown = |t: &mut Term, damage| {
        r.render(t, &mut c.target(), damage);
        let mut n = 0;
        for j in 0..dh {
            for i in 0..dw {
                let (sx, sy) = source(e, i, j);
                let (x, y) = (COL * cw + dx + i, ROW * ch + dy + j);
                let p =
                    c.pixels[usize::try_from(y).unwrap() * c.width + usize::try_from(x).unwrap()];
                n += usize::from(p == texel(sx, sy));
            }
        }
        n
    };
    let all = usize::try_from(dw * dh).unwrap();
    assert_eq!(shown(&mut t, Damage::Full), 0, "set: under the cells");
    feed(&mut t, b"\x1b]11;rgb:30/00/30\x1b\\");
    assert_eq!(
        shown(&mut t, Damage::Incremental),
        all,
        "OSC 11 to their colour: the image shows"
    );
    feed(&mut t, b"\x1b]111\x1b\\");
    assert_eq!(
        shown(&mut t, Damage::Incremental),
        0,
        "OSC 111: under them again"
    );
}

/// **The owner's pick, "Picture shows dimmed" (2026-10-16)**: on the
/// translucent window a coloured cell's background is composited over a
/// deep image at its own alpha, as it is over the desktop where there is
/// no image, as kitty blends the non-default backgrounds over the images
/// below them (`[[source: kitty-cell-slang]]`). So the image shows tinted
/// through the coloured cells, fully through the default ones; where the
/// image is clear the coloured cell is the frame without the image, not
/// its background twice; and an opaque coloured cell -- `[glass]
/// app_backgrounds = "opaque"` -- is its background, as before.
#[test]
fn on_the_glass_a_coloured_cell_is_blended_over_a_deep_image() {
    let dimmed = Theme {
        bg_alpha: baud_domain::Chan::from_u8(56),
        ..Theme::dark()
    };
    let red = dimmed.palette[1].0 & 0xFF_FFFF | 56 << 24;
    let text = "\x1b[3;5H\x1b[41m  \x1b[0m  ";
    let (wrong, all) = deep_cells(
        text,
        &dimmed,
        0xFF,
        [Deep::Over(red), Deep::Over(red), Deep::Texel, Deep::Texel],
    );
    assert_eq!(wrong, 0, "dimmed red over the image, of {all}");
    let (wrong, all) = deep_cells(text, &dimmed, 0, [Deep::Plain; 4]);
    assert_eq!(wrong, 0, "a clear image: the frame without it, of {all}");
    let opaque = Theme {
        app_bg: baud_render::AppBg::Opaque,
        ..dimmed.clone()
    };
    let (wrong, all) = deep_cells(text, &opaque, 0xFF, RED_SET);
    assert_eq!(wrong, 0, "an opaque coloured cell replaces, of {all}");
}

/// **A repainted row examines only the placements on it**: a picture
/// placed one cell at a time, 20 by 6 at `z=-1` with one more three rows
/// tall, over a 50-row screen, and a full frame. Each placement is
/// examined twice for each row it covers -- the cells the row's
/// placements reach, then the draw -- and never for a row it is not
/// on, where every repainted row once asked every placement. A frame
/// repainting one row walks them all instead, cheaper than an index.
#[test]
fn a_repainted_row_examines_only_the_placements_on_it() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let m = r.metrics();
    let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(
        ImagePx::of(u16::try_from(cw).unwrap()),
        ImagePx::of(u16::try_from(ch).unwrap()),
    );
    feed(&mut t, b"\x1b[?25l");
    transmit(&mut t, 1, texel);
    let mut p = 1;
    for y in 0..6 {
        for x in 0..20 {
            feed(
                &mut t,
                format!(
                    "\x1b[{};{}H\x1b_Ga=p,i=1,p={p},x={},y={},w=1,h=1,c=1,r=1,z=-1,C=1,q=2\x1b\\",
                    y + 3,
                    x + 5,
                    x % IW,
                    y % IH
                )
                .as_bytes(),
            );
            p += 1;
        }
    }
    feed(
        &mut t,
        b"\x1b[20;5H\x1b_Ga=p,i=1,p=999,r=3,z=-1,C=1,q=2\x1b\\",
    );
    assert_eq!(t.images().placements().count(), 121, "the premise: placed");
    let frame = {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full)
    };
    assert_eq!(
        frame.rows_dirty.value(),
        u64::from(WS.ws_row),
        "every row repainted"
    );
    assert_eq!(
        frame.places_examined.value(),
        2 * (120 + 3),
        "each placement examined twice for each row it covers"
    );
    // One row repainted -- a placement put again with another source
    // pixel -- is too few to index, and walks every placement.
    feed(
        &mut t,
        b"\x1b[3;5H\x1b_Ga=p,i=1,p=1,x=1,y=1,w=1,h=1,c=1,r=1,z=-1,C=1,q=2\x1b\\",
    );
    let frame = {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Incremental)
    };
    assert_eq!(
        frame.rows_dirty.value(),
        1,
        "the premise: one row repainted"
    );
    assert_eq!(
        frame.places_examined.value(),
        2 * 121,
        "one row, every placement"
    );
}
