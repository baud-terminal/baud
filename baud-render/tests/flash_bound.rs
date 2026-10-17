//! The trails' flash bound in the renderer (note AL, option O2): a repeat
//! pass's records, the cap they carry and the ground it is read from, and
//! the CPU path's quad. The thresholds are WCAG 2.2's (`[[source: wcag22]]`):
//! a general transition is a change in relative luminance of 0.10 or more
//! where the darker state is below 0.80.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Col, Cols, ImageBudget, ImagePx, ReadLen, Row, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::effect::{Effect, Ignition, Seed, Vfx};
use baud_render::{
    Argb, Blend, Damage, Kind, Pt, PxSize, Renderer, Segment, Shape, Smear, Stack, Strength,
    Target, Theme, TrailLook, under_threshold, under_threshold_over,
};
use support::font;

const WS: WinSize = WinSize {
    ws_col: 30,
    ws_row: 10,
};

fn px(p: Px) -> usize {
    usize::try_from(p.value()).unwrap()
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
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

/// A frame on the CPU path, which reads the styles and so sets the cap.
fn paint(r: &mut Renderer<'_, '_>, t: &mut Term<'_>) -> Vec<u32> {
    let (w, h) = r.pixel_size();
    let mut pixels = vec![0u32; px(w) * px(h)];
    let mut target = Target::new(&mut pixels, w, w, h).unwrap();
    r.render(t, &mut target, Damage::Full);
    pixels
}

/// A quad `t` seconds into a move from `from` to the block cursor at `to`.
fn smear(r: &Renderer<'_, '_>, from: (usize, usize), to: (usize, usize), t: f32) -> Smear {
    let style = baud_core::CursorStyle::SteadyBlock;
    let rect =
        |(row, col): (usize, usize)| r.cursor_rect(Row::from_index(row), Col::at_index(col), style);
    let hole = rect(to);
    Smear {
        corners: Segment::new(rect(from).corners(), hole).at(t),
        hole,
        shape: Shape::Quad,
        strength: Strength::Full,
    }
}

/// An effect riding that quad, `t` seconds after it lit.
fn vfx(r: &Renderer<'_, '_>, effect: Effect, t: f32) -> Vfx {
    let emitter = smear(r, (1, 2), (7, 20), t);
    let from = emitter.corners.0;
    let tail = Pt {
        x: (from[2].x + from[0].x) * 0.5,
        y: (from[0].y + from[1].y) * 0.5,
    };
    let unit = f32::from(u16::try_from(r.metrics().cell_h.value()).unwrap());
    Vfx {
        effect,
        emitter,
        ignition: Ignition::ignite(tail, emitter.hole, Seed(5), unit).at(t),
        axis: Pt { x: 1.0, y: 0.0 },
    }
}

fn repeat(look: TrailLook) -> TrailLook {
    match look {
        TrailLook::Vfx(v) => TrailLook::Vfx(Vfx {
            emitter: Smear {
                strength: Strength::UnderThreshold,
                ..v.emitter
            },
            ..v
        }),
        TrailLook::Smear(s) => TrailLook::Smear(Smear {
            strength: Strength::UnderThreshold,
            ..s
        }),
        TrailLook::Ghost(_) => unreachable!("no ghost here"),
    }
}

/// Whether two records' rectangles share a pixel.
fn overlap(a: &baud_render::Instance, b: &baud_render::Instance) -> bool {
    let span = |p: i16, n: u16| (i32::from(p), i32::from(p) + i32::from(n));
    let (ax, ay, bx, by) = (
        span(a.x.0, a.w.0),
        span(a.y.0, a.h.0),
        span(b.x.0, b.w.0),
        span(b.y.0, b.h.0),
    );
    ax.0 < bx.1 && bx.0 < ax.1 && ay.0 < by.1 && by.0 < ay.1
}

fn opaque(r: u8, g: u8, b: u8) -> Argb {
    Argb::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b))
}

/// **A repeat pass draws one record a pixel, none over the cap**, for
/// every effect at three ages: its bands, which are disjoint, and not its
/// particles, which stack over them and over each other, so that under
/// source-over their alphas would compound past any one record's cap. The
/// cap is the cursor's colour's over the page under it, the only ground on
/// a blank screen. The controls: the same look at full strength has records
/// that overlap and alphas over the cap.
#[test]
fn a_repeat_pass_draws_one_record_a_pixel_under_the_cap() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l");
    let mut r = renderer(&faces);
    let _ = paint(&mut r, &mut t);
    let (ink, page) = (Theme::dark().fg, Theme::dark().bg);
    let k = under_threshold(
        ink.with_alpha(Chan::from_u8(0xFF)),
        page.with_alpha(Chan::from_u8(0xFF)),
    );
    assert!(
        k.get() < 0xFF,
        "the control: this ink over this page has a cap"
    );
    let mut stacked = 0;
    for effect in Effect::ALL {
        for age in [0.02f32, 0.1, 0.3] {
            let full = TrailLook::Vfx(vfx(&r, effect, age));
            r.set_trail(Some(full));
            let _ = paint(&mut r, &mut t);
            let records = r.trail().to_vec();
            assert!(
                records.iter().any(|i| i.color.a().get() > k.get()),
                "{effect:?} at {age}: the control, a full pass over the cap"
            );
            if records.iter().any(|i| i.kind == Kind::Particle) {
                stacked += 1;
            }
            r.set_trail(Some(repeat(full)));
            let _ = paint(&mut r, &mut t);
            assert_eq!(r.flash_cap(), k, "{effect:?} at {age}: the page's cap");
            let records = r.trail();
            assert!(!records.is_empty(), "{effect:?} at {age}: a repeat draws");
            for (n, a) in records.iter().enumerate() {
                assert!(
                    a.color.a().get() <= k.get(),
                    "{effect:?} at {age}: a record at {} over the cap {}",
                    a.color.a().get(),
                    k.get()
                );
                for b in &records[n + 1..] {
                    assert!(
                        !overlap(a, b),
                        "{effect:?} at {age}: two records share a pixel, {:?} and {:?}",
                        a.kind,
                        b.kind
                    );
                }
            }
        }
    }
    assert!(stacked > 0, "the control: full passes draw particles");
}

/// **The cap is read from the cells under the pass**: a quad over cells
/// whose background is `#CD0000` takes the black cursor's cap over that red
/// (124 of 255), where the same pass over the page takes the page's (255:
/// black over `#101010` never flashes); over `X`s in the default colour it
/// takes the least cap over every mixture a glyph edge draws of that text
/// over the page, which is under both ends'.
#[test]
fn a_repeat_pass_is_capped_against_the_cells_under_it() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let black = opaque(0, 0, 0);
    let page = Theme::dark().bg.with_alpha(Chan::from_u8(0xFF));
    let text = Theme::dark().fg.with_alpha(Chan::from_u8(0xFF));
    let mut caps = Vec::new();
    for under in ["", "\x1b[48;2;205;0;0m", "X"] {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b[?25l\x1b]12;rgb:00/00/00\x07");
        if under == "X" {
            for row in 1..9 {
                feed(
                    &mut t,
                    format!("\x1b[{};1H{}", row, "X".repeat(30)).as_bytes(),
                );
            }
        } else if !under.is_empty() {
            for row in 1..9 {
                feed(
                    &mut t,
                    format!("\x1b[{row};1H{under}{}\x1b[0m", " ".repeat(30)).as_bytes(),
                );
            }
        }
        let mut r = renderer(&faces);
        let _ = paint(&mut r, &mut t);
        r.set_trail(Some(repeat(TrailLook::Smear(smear(
            &r,
            (1, 2),
            (6, 20),
            0.05,
        )))));
        let _ = paint(&mut r, &mut t);
        caps.push(r.flash_cap());
    }
    assert_eq!(caps[0], under_threshold(black, page), "over the page");
    assert_eq!(
        caps[0].get(),
        0xFF,
        "the control: black never flashes on #101010"
    );
    assert_eq!(
        caps[1],
        under_threshold(black, opaque(0xCD, 0, 0)),
        "over red cells"
    );
    assert_eq!(caps[1].get(), 124, "note AL's cap for black on red");
    let mixed = under_threshold_over(black, text, page);
    assert_eq!(caps[2], mixed, "over text");
    assert!(
        mixed.get() <= under_threshold(black, text).get().min(caps[0].get()),
        "a mixture's cap is under both ends'"
    );
}

/// WCAG's relative luminance of a word's three bytes.
fn lum(p: u32) -> f64 {
    let [_, r, g, b] = p.to_be_bytes();
    let lin = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

/// **The CPU path's quad carries the cap**: where the app has no GPU, or
/// `BAUD_GPU=0`, every trail is the quad, painted by `Target::quad` from the
/// look rather than from its record, so the record's cap alone would bound
/// nothing there. No pixel of a repeat quad over the page is a general
/// transition from the frame without it; the control, the full quad, makes
/// them.
#[test]
fn a_repeat_quad_on_the_cpu_path_makes_no_transition() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l");
    let mut r = renderer(&faces);
    let bare = paint(&mut r, &mut t);
    let transitions = |frame: &[u32]| {
        frame
            .iter()
            .zip(&bare)
            .filter(|&(&p, &q)| {
                let (a, b) = (lum(p), lum(q));
                (a - b).abs() >= 0.10 && a.min(b) < 0.80
            })
            .count()
    };
    let quad = TrailLook::Smear(smear(&r, (1, 2), (7, 20), 0.05));
    r.set_trail(Some(quad));
    let full = paint(&mut r, &mut t);
    assert!(transitions(&full) > 0, "the control: the full quad flashes");
    r.set_trail(Some(repeat(quad)));
    let repeated = paint(&mut r, &mut t);
    assert!(
        repeated.iter().zip(&bare).any(|(p, q)| p != q),
        "the control: the repeat quad draws"
    );
    assert_eq!(transitions(&repeated), 0, "a repeat quad's pixel flashes");
}

/// **The cap's work is bounded a frame, and the bound stays sound while it
/// is** (Jev, decisions 728/729): over more than 64 distinct grounds -- a
/// row of cells in 70 background colours -- a repeat pass draws nothing, a
/// cap of 0, rather than scan every one, which over a picture cost 42-97 ms
/// a frame. Over a few grounds, two new caps are scanned a frame and the
/// pass draws nothing until all are: three colours under it read 0 on the
/// first frame and their least cap on the second. The control: the same
/// three colours' least cap, scanned directly.
#[test]
fn a_repeat_pass_over_many_grounds_draws_nothing() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let black = opaque(0, 0, 0);
    let colour = |n: u8| opaque(0x40 + n, 0x80, 0x20 + 2 * n);
    let mut caps = Vec::new();
    for count in [70u8, 3] {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b[?25l\x1b]12;rgb:00/00/00\x07");
        for row in 1..9 {
            let mut line = format!("\x1b[{row};1H");
            for col in 0..30u8 {
                let c = colour((col + 30 * (row - 1)) % count);
                line.push_str(&format!(
                    "\x1b[48;2;{};{};{}m ",
                    c.r().get(),
                    c.g().get(),
                    c.b().get()
                ));
            }
            feed(&mut t, format!("{line}\x1b[0m").as_bytes());
        }
        let mut r = renderer(&faces);
        let _ = paint(&mut r, &mut t);
        // A quad over the eight coloured rows whole, the cursor below them.
        let style = baud_core::CursorStyle::SteadyBlock;
        let (a, b) = (
            r.cursor_rect(Row::from_index(1), Col::at_index(0), style),
            r.cursor_rect(Row::from_index(8), Col::at_index(29), style),
        );
        let hole = r.cursor_rect(Row::from_index(9), Col::at_index(29), style);
        let over = baud_render::Rect {
            left: a.left,
            top: a.top,
            right: b.right,
            bottom: b.bottom,
        };
        r.set_trail(Some(repeat(TrailLook::Smear(Smear {
            corners: over.corners(),
            hole,
            shape: Shape::Quad,
            strength: Strength::Full,
        }))));
        let _ = paint(&mut r, &mut t);
        let first = r.flash_cap();
        let _ = paint(&mut r, &mut t);
        caps.push((first, r.flash_cap()));
    }
    assert_eq!(caps[0], (Chan::from_u8(0), Chan::from_u8(0)), "70 grounds");
    let least = (0..3)
        .map(|n| under_threshold(black, colour(n)).get())
        .min()
        .unwrap();
    assert!(least > 0, "the control: these grounds have a cap");
    assert_eq!(
        caps[1].0.get(),
        0,
        "three grounds, two scanned on frame one"
    );
    assert_eq!(caps[1].1.get(), least, "every ground scanned on frame two");
}

fn b64(data: &[u8]) -> Vec<u8> {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    for q in data.chunks(3) {
        let mut b = [0u8; 3];
        b[..q.len()].copy_from_slice(q);
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        for k in 0..4 {
            if k <= q.len() {
                out.push(A[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]);
            } else {
                out.push(b'=');
            }
        }
    }
    out
}

/// One kitty graphics command.
fn apc(t: &mut Term<'_>, ctrl: &str, payload: &[u8]) {
    let mut v = b"\x1b_G".to_vec();
    v.extend_from_slice(ctrl.as_bytes());
    if !payload.is_empty() {
        v.push(b';');
        v.extend_from_slice(payload);
    }
    v.extend_from_slice(b"\x1b\\");
    feed(t, &v);
}

/// **An image under the text is a ground the cap cannot read**, and a
/// repeat pass over one draws nothing (Jev, decisions 728/729, the route a
/// picture takes): the cells under a kitty image hold the page's styles
/// while the pixels the pass lands on are the image's, so the page's cap
/// over a `#404040` image made 672 general transitions on a 30 by 10 grid.
/// An image over the text hides the pass, which keeps the page's cap: the
/// control for the layer. The control for the pass: the same quad on a
/// bare page draws at the page's cap.
#[test]
fn a_repeat_pass_over_an_image_under_the_text_draws_nothing() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let page = under_threshold(
        Theme::dark().fg.with_alpha(Chan::from_u8(0xFF)),
        Theme::dark().bg.with_alpha(Chan::from_u8(0xFF)),
    );
    assert!(page.get() > 0, "the control: the page has a cap");
    // No image, then one over the text, under it, below the backgrounds.
    for (z, cap) in [
        (None, page),
        (Some(1i32), page),
        (Some(-1), Chan::from_u8(0)),
        (Some(-1_073_741_825), Chan::from_u8(0)),
    ] {
        let mut r = renderer(&faces);
        let m = r.metrics();
        let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
        let mut t = Term::new(owned.storage(), &WS);
        t.set_cell_size(
            ImagePx::of(u16::try_from(m.cell_w.value()).unwrap()),
            ImagePx::of(u16::try_from(m.cell_h.value()).unwrap()),
        );
        feed(&mut t, b"\x1b[?25l");
        if let Some(z) = z {
            let grey = [0x40, 0x40, 0x40, 0xFF].repeat(24);
            apc(&mut t, "a=t,f=32,s=6,v=4,i=1,q=2", &b64(&grey));
            feed(&mut t, b"\x1b[2;1H");
            apc(&mut t, &format!("a=p,i=1,p=1,q=2,c=30,r=8,z={z}"), b"");
            feed(&mut t, b"\x1b[1;1H");
        }
        let bare = paint(&mut r, &mut t);
        r.set_trail(Some(repeat(TrailLook::Smear(smear(
            &r,
            (1, 2),
            (7, 20),
            0.05,
        )))));
        let _ = paint(&mut r, &mut t);
        let frame = paint(&mut r, &mut t);
        let transitions = frame
            .iter()
            .zip(&bare)
            .filter(|&(&p, &q)| {
                let (a, b) = (lum(p), lum(q));
                (a - b).abs() >= 0.10 && a.min(b) < 0.80
            })
            .count();
        let at = format!("z {z:?}");
        assert_eq!(r.flash_cap(), cap, "the cap, {at}");
        assert_eq!(
            transitions, 0,
            "a repeat pass flashes over the ground, {at}"
        );
        let drew = frame.iter().zip(&bare).any(|(p, q)| p != q);
        assert_eq!(drew, z.is_none(), "it draws only on a bare page, {at}");
    }
}
