//! **The visual bell in a colour** (`baud_render::bell`): a `BEL` under the
//! config's `bell = "flash"` draws the colour a program set with kitty's
//! `OSC 21 ; visual_bell=` as one solid over the whole screen, kitty's
//! tint; with none set, or the automatic one, it draws the other video, as
//! a `?5` pair no frame drew does.

mod support;

use core::time::Duration;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Cols, ReadLen, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::{
    Argb, BellFlash, BellKind, BellShown, Blend, Damage, Kind, PxSize, Rang, Renderer, Stack,
    Theme, VideoFlash,
};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 3,
    ws_col: 12,
};

const LENGTH: Duration = Duration::from_millis(100);

/// A screen with a default cell, a coloured one, palette 0 and 7, and a
/// second row.
const SCENE: &[u8] = b"A\x1b[41mB\x1b[30;47mC\x1b[0m\r\nD\x1b[2;12H";

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn renderer<'f>(faces: &'f [Face<'f>], blend: Blend) -> Renderer<'f, 'f> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        blend,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap()
}

/// The bell rung at 0 and shown at `at`, as the driver plans it: how it
/// shows from what the program set, and what the frame draws.
fn shown(t: &Term<'_>, at: Duration) -> BellShown {
    let (b, _) = BellFlash::new(LENGTH).at(Duration::ZERO, Rang::Yes, BellKind::of(t));
    b.at(at, Rang::No, BellKind::of(t)).1
}

/// `ink` over the opaque `dst` at `a`, as a frame mixes a solid in encoded
/// numbers.
fn native(ink: Argb, dst: Argb, a: Chan) -> Argb {
    let a = u32::from(a.get());
    let mix = |s: Chan, d: Chan| {
        let v = (u32::from(s.get()) * a + u32::from(d.get()) * (255 - a) + 127) / 255;
        Chan::from_u8(u8::try_from(v).unwrap())
    };
    Argb::rgb(
        mix(ink.r(), dst.r()),
        mix(ink.g(), dst.g()),
        mix(ink.b(), dst.b()),
    )
}

/// **A colour set draws that colour over every pixel**, at the fade's
/// alpha, kitty's 0.6 for red at the peak -- on a full frame and on the
/// incremental one after, which repaints every row while the tint changes
/// -- and the frame after it is over is the frame before it rang.
#[test]
fn a_coloured_bell_draws_its_colour_over_every_pixel() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = renderer(&faces, Blend::Native);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, SCENE);
    let mut c = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut c, Damage::Full);
    let base = c.pixels.clone();
    feed(&mut t, b"\x1b]21;visual_bell=#ff0000\x07");
    support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
    assert_eq!(c.pixels, base, "setting the colour draws nothing");
    let red = Argb(0xFFFF_0000);
    for (at, alpha) in [(50, 153), (25, 76)] {
        let s = shown(&t, Duration::from_millis(at));
        let tint = s.tint().expect("a tint");
        assert_eq!((tint.colour, tint.alpha.get()), (red, alpha), "{at} ms");
        assert_eq!(s.video(), VideoFlash::Off, "not the other video");
        r.set_flash(s.video());
        r.set_bell_tint(s.tint());
        support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
        let want: Vec<u32> = base
            .iter()
            .map(|&p| native(red, Argb(p), Chan::from_u8(alpha)).0)
            .collect();
        let off = c.pixels.iter().zip(&want).filter(|(a, b)| a != b).count();
        assert_eq!(off, 0, "{at} ms: every pixel is the colour over the frame");
        // And the rows a presenter is told changed, the Linux driver's
        // damage, are every row.
        let mut rows = vec![false; c.height];
        for b in r.painted() {
            let px = |p: Px| usize::try_from(p.value()).unwrap();
            let (top, bottom) = (px(b.top), px(b.bottom));
            rows[top..bottom.min(c.height)].fill(true);
        }
        assert!(rows.iter().all(|&r| r), "{at} ms: every row is painted");
    }
    r.set_bell_tint(shown(&t, LENGTH).tint());
    support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
    assert_eq!(c.pixels, base, "over at the length, nothing left");
}

/// **No colour, or the automatic one, draws the other video**: the frame
/// `?5 h` draws, with no solid over it.
#[test]
fn an_automatic_bell_draws_the_other_video() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let frame = |bytes: &[u8], bell: Option<Duration>| {
        let mut r = renderer(&faces, Blend::Native);
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, bytes);
        if let Some(at) = bell {
            let s = shown(&t, at);
            r.set_flash(s.video());
            r.set_bell_tint(s.tint());
        }
        let mut c = support::Canvas::for_renderer(&r);
        support::paint(&mut r, &mut t, &mut c, Damage::Full);
        c.pixels
    };
    let reversed = frame(&[b"\x1b[?5h", SCENE].concat(), None);
    assert_ne!(frame(SCENE, None), reversed, "the control: ?5 changes it");
    let at = Some(Duration::from_millis(50));
    assert_eq!(frame(SCENE, at), reversed, "none set");
    let automatic = [b"\x1b]21;visual_bell=\x07", SCENE].concat();
    assert_eq!(frame(&automatic, at), reversed, "the automatic colour");
    let reset = [b"\x1b]21;visual_bell=red\x07\x1b]21;visual_bell\x07", SCENE].concat();
    assert_eq!(frame(&reset, at), reversed, "a colour reset");
    let red = [b"\x1b]21;visual_bell=red\x07", SCENE].concat();
    assert_ne!(frame(&red, at), reversed, "the control: a colour is not it");
}

/// **Under `Blend::Linear` the solid mixes in linear light**, as kitty's
/// layered path mixes its tint: a mid-grey page under white at 0.6 is
/// lighter than the encoded mix.
#[test]
fn under_linear_blending_the_tint_mixes_in_linear_light() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let corner = |blend: Blend| {
        let mut r = renderer(&faces, blend);
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b]21;visual_bell=#ffffff\x07");
        let s = shown(&t, Duration::from_millis(50));
        r.set_bell_tint(s.tint());
        let mut c = support::Canvas::for_renderer(&r);
        support::paint(&mut r, &mut t, &mut c, Damage::Full);
        c.pixels[c.pixels.len() - 1] & 0xFF
    };
    let (native, linear) = (corner(Blend::Native), corner(Blend::Linear));
    assert!(
        linear > native,
        "linear {linear:#x} over native {native:#x}"
    );
}

/// **The solid is the first of baud's own overlays**: the instance and
/// cell paths draw [`Renderer::chrome`]'s records last, and the bell's
/// leads them, under the scrollbar's thumb.
#[test]
fn the_bell_is_drawn_first_of_the_overlays() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = renderer(&faces, Blend::Native);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, SCENE);
    feed(&mut t, b"\x1b]21;visual_bell=#00ff00\x07");
    r.set_bell_tint(shown(&t, Duration::from_millis(50)).tint());
    r.convert(&t, Rows::of_index(0), Damage::Full);
    let all: Vec<_> = r.instances().copied().collect();
    let chrome = usize::try_from(r.chrome().value()).unwrap();
    let tail = &all[all.len() - chrome..];
    assert_eq!(r.bell_tint().len(), 1);
    assert_eq!(tail[0], r.bell_tint()[0], "first of the chrome");
    let (w, h) = r.pixel_size();
    let b = r.bell_tint()[0];
    assert_eq!(b.kind, Kind::Solid);
    assert_eq!(
        (u32::from(b.w.0), u32::from(b.h.0)),
        (w.value(), h.value()),
        "over the whole screen"
    );
}
