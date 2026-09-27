//! A visual bell whose halves no frame drew: [`VideoFlash`].
//!
//! `tput flash` writes `?5h` and `?5l` in one read, so the mode reads the
//! same before and after and no frame would ever show the reversed screen.
//! The driver sees it through `Term::reverse_flips` and shows the other video
//! for the entry's length; this is the renderer's half, which draws a flash
//! by the one rule `?5` is drawn by (`Video`, xterm's `ReverseVideo`).
//!
//! **A flash is the opposite of the mode, not reverse video**: over a
//! reversed screen it shows the normal one. Every test renders at a
//! translucent `bg_alpha`, where the alpha half of the rule is visible.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::{
    Argb, Blend, Damage, Hover, Look, PxSize, Renderer, Resolved, Scroll, Scroller, Stack, Theme,
    Trail, TrailLook, Transition, VideoFlash, Widths, Window,
};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 2,
    ws_col: 12,
};

/// `[glass] dim_dark`'s default, 0.22 of 255 rounded.
const DIM: u8 = 56;

fn rgb(c: Argb) -> u32 {
    c.0 & 0x00FF_FFFF
}

fn dim_theme() -> Theme {
    let mut t = Theme::dark();
    t.bg_alpha = Chan::from_u8(DIM);
    t
}

fn renderer<'f>(faces: &'f [Face<'f>], ws: &WinSize) -> Renderer<'f, 'f> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        dim_theme(),
        Blend::Native,
        Cols::of_index(usize::from(ws.ws_col)),
        Rows::of_index(usize::from(ws.ws_row)),
    )
    .unwrap()
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// A screen with a default cell, a coloured one, palette 0 and 7, and a
/// second row, so a swap of the defaults and of the palette both show.
const SCENE: &[u8] = b"A\x1b[41mB\x1b[30;47mC\x1b[0m\r\nD\x1b[2;12H";

/// One full frame of `bytes` under `flash`, as a digest.
fn frame(bytes: &[u8], flash: VideoFlash) -> u64 {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = renderer(&faces, &WS);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, bytes);
    r.set_flash(flash);
    let mut c = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut c, Damage::Full);
    c.digest()
}

/// The flash draws the frame `?5` draws, in both directions, and differs
/// from the frame without it.
#[test]
fn a_flash_draws_the_other_video() {
    let normal = frame(SCENE, VideoFlash::Off);
    let reversed = frame(&[b"\x1b[?5h", SCENE].concat(), VideoFlash::Off);
    assert_ne!(normal, reversed, "the control: ?5 changes the frame");
    assert_eq!(
        frame(SCENE, VideoFlash::On),
        reversed,
        "a flash over a normal screen is the reversed screen"
    );
    assert_eq!(
        frame(&[b"\x1b[?5h", SCENE].concat(), VideoFlash::On),
        normal,
        "a flash over a reversed screen is the normal one"
    );
}

/// The cell path's colour table is resolved by the same rule, so a flash
/// reaches the shader the way `?5` does.
#[test]
fn the_resolved_table_follows_the_flash() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let table = |bytes: &[u8], flash: VideoFlash| {
        let mut r = renderer(&faces, &WS);
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, bytes);
        r.set_flash(flash);
        let mut out = vec![Resolved::default(); t.styles().len()];
        r.resolve_styles_into(&t, &mut out);
        out
    };
    let normal = table(SCENE, VideoFlash::Off);
    let reversed = table(&[b"\x1b[?5h", SCENE].concat(), VideoFlash::Off);
    assert_ne!(normal, reversed, "the control");
    assert_eq!(table(SCENE, VideoFlash::On), reversed);
    assert_eq!(
        table(&[b"\x1b[?5h", SCENE].concat(), VideoFlash::On),
        normal
    );
}

/// **A flash moves no cell word and no style slot, so it has to repaint on
/// incremental damage**, the way a `?5` flip does. Its start equals a full
/// repaint of the reversed screen, and its end is the frame before it.
#[test]
fn a_flash_starts_and_ends_on_incremental_damage() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = renderer(&faces, &WS);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut c = support::Canvas::for_renderer(&r);
    feed(&mut t, SCENE);
    support::paint(&mut r, &mut t, &mut c, Damage::Full);
    let normal = c.digest();
    let quiet = support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
    assert_eq!(
        quiet.rows_dirty.value(),
        0,
        "a quiet frame repaints nothing"
    );

    r.set_flash(VideoFlash::On);
    let f = support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
    assert_eq!(
        f.rows_dirty.value(),
        f.rows.value(),
        "the start repaints every row"
    );
    let on = frame(&[b"\x1b[?5h", SCENE].concat(), VideoFlash::Off);
    assert_eq!(c.digest(), on, "and equals the reversed screen, full");

    let held = support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
    assert_eq!(held.rows_dirty.value(), 0, "a flash held repaints nothing");

    r.set_flash(VideoFlash::Off);
    let f = support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
    assert_eq!(
        f.rows_dirty.value(),
        f.rows.value(),
        "the end repaints every row"
    );
    assert_eq!(c.digest(), normal, "and is the frame before");
}

/// The margin a driver reads before it renders follows the flash too, so
/// the Linux strips past the grid flash with it.
#[test]
fn the_margin_follows_the_flash() {
    let theme = dim_theme();
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = renderer(&faces, &WS);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"A");
    for (bytes, flash, margin) in [
        (&b""[..], VideoFlash::Off, theme.bg),
        (b"", VideoFlash::On, theme.fg),
        (b"\x1b[?5h", VideoFlash::On, theme.bg),
        (b"", VideoFlash::Off, theme.fg),
        (b"\x1b[?5l", VideoFlash::Off, theme.bg),
    ] {
        feed(&mut t, bytes);
        r.set_flash(flash);
        let before = r.clear_color_of(&t);
        r.convert(&t, Rows::ZERO, Damage::Incremental);
        assert_eq!(before, r.clear_color(), "{bytes:?} {flash:?}");
        assert_eq!(rgb(r.clear_color()), rgb(margin), "{bytes:?} {flash:?}");
    }
}

const TALL: WinSize = WinSize {
    ws_row: 10,
    ws_col: 30,
};

/// What is drawn in the foreground or the background -- the scrollbar, the
/// trail, the screen fade -- follows the flash on the frame it starts, and
/// back on the frame it ends. They are set before a convert, so the flash
/// has to rebuild the ones already set, as a `?5` flip does.
#[test]
fn the_overlays_follow_the_flash() {
    let theme = dim_theme();
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = renderer(&faces, &TALL);
    let mut owned = Owned::new(&TALL);
    let mut t = Term::new(owned.storage(), &TALL);
    let lines: Vec<u8> = (0..40)
        .flat_map(|i| format!("line {i}\r\n").into_bytes())
        .collect();
    feed(&mut t, &lines);
    r.set_scrollbar(Some(Scroll {
        window: Window {
            retained: Rows::of_count(t.scrollback().len()),
            back: Rows::of_count(3),
            rows: Rows::of_index(usize::from(TALL.ws_row)),
        },
        look: Look {
            shown: Chan::from_u8(255),
            hover: Hover::Rest,
        },
        widths: Widths {
            rest: Px::of(12),
            hovered: Px::of(22),
            inset: Px::of(6),
            strip: Px::of(34),
        },
        scroller: Scroller::Overlay,
    }));
    r.set_trail(Some(TrailLook::Ghost(Trail {
        from: (Row::from_index(1), Col::at_index(1)),
        to: (Row::from_index(4), Col::at_index(20)),
        progress: 0.5,
        origin_dy: 0.0,
        lift: Px::of(0),
    })));
    r.set_screen_fade(Some(Transition { progress: 0.5 }));
    let colours = |r: &Renderer<'_, '_>| {
        let fg: Vec<u32> = r
            .scrollbar()
            .iter()
            .chain(r.trail())
            .map(|i| rgb(i.color))
            .collect();
        let bg: Vec<u32> = r.screen_fade().iter().map(|i| rgb(i.color)).collect();
        (fg, bg)
    };
    r.convert(&t, Rows::ZERO, Damage::Full);
    let (fg, bg) = colours(&r);
    assert!(
        !fg.is_empty() && !bg.is_empty(),
        "the control: all three drawn"
    );
    assert!(fg.iter().all(|&c| c == rgb(theme.fg)));
    assert!(bg.iter().all(|&c| c == rgb(theme.bg)));

    r.set_flash(VideoFlash::On);
    r.convert(&t, Rows::ZERO, Damage::Incremental);
    let (fg, bg) = colours(&r);
    assert!(fg.iter().all(|&c| c == rgb(theme.bg)), "flashed foreground");
    assert!(bg.iter().all(|&c| c == rgb(theme.fg)), "flashed background");

    r.set_flash(VideoFlash::Off);
    r.convert(&t, Rows::ZERO, Damage::Incremental);
    let (fg, bg) = colours(&r);
    assert!(
        fg.iter().all(|&c| c == rgb(theme.fg)),
        "back after the flash"
    );
    assert!(bg.iter().all(|&c| c == rgb(theme.bg)));
}
