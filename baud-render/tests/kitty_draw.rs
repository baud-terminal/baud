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
use baud_font::Face;
use baud_render::{Damage, Theme};
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
