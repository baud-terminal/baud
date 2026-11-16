//! **The CPU and instance paths draw a window over history the core froze in
//! the renditions it was written in** (`baud_render::Lent`): the path's own
//! copy of the window gives each frozen cell a slot of its table no other
//! cell of the frame names, as the cell path's copy does. The Linux driver
//! draws on this path alone.
//!
//! `[[source: dec-std-070]]`'s SGR, page 5-104, note 3: "In no way does the
//! selection of current rendition affect the rendition of characters already
//! entered into the display."
//!
//! Each test names more renditions than the 1,024-slot table holds past its
//! 7/8 load, so it freezes, and checks the control that it did. The
//! reference is a terminal provisioned at 400x150, whose 65,536-slot table
//! never fills here, drawn at the same window.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Damage, PxSize, Renderer, Stack, Theme};
use support::{Canvas, font};

const WS: WinSize = WinSize {
    ws_row: 10,
    ws_col: 40,
};

/// The app's provisioning, whose table holds 65,536 renditions.
const BIG: WinSize = WinSize {
    ws_row: 150,
    ws_col: 400,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    for c in bytes.chunks(4096) {
        t.write_at(BytePos::ZERO, c);
        t.feed(BytePos::ZERO, ReadLen::from_index(c.len()));
    }
}

/// `lines` lines, every cell its own truecolour background, from `first`.
fn flood(first: usize, lines: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for l in 0..lines {
        for x in 0..40 {
            let i = first + l * 40 + x;
            let b = i.to_le_bytes();
            out.extend(format!("\x1b[0;48;2;{};{};77mq", b[0], b[1]).as_bytes());
        }
        out.extend(b"\x1b[0m\r\n");
    }
    out
}

/// `lines` lines, every cell its own foreground and background and every
/// fifth bold, each line the same letters, from `first`.
fn flood_both(first: usize, lines: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for l in 0..lines {
        for x in 0..40 {
            let i = first + l * 40 + x;
            let b = i.to_le_bytes();
            let bold = match i % 5 {
                0 => ";1",
                _ => "",
            };
            let letter = char::from(b'a' + u8::try_from(x % 26).unwrap());
            out.extend(
                format!(
                    "\x1b[0{bold};38;2;{};{};9;48;2;9;{};{}m{letter}",
                    b[0], b[1], b[1], b[0]
                )
                .as_bytes(),
            );
        }
        out.extend(b"\x1b[0m\r\n");
    }
    out
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

fn backgrounds(t: &mut Term<'_>, faces: &[Face<'_>], back: usize) -> Vec<u32> {
    let mut r = renderer(faces);
    let mut canvas = Canvas::for_renderer(&r);
    let mut target = canvas.target();
    let _ = r.render_at(t, Rows::of_count(back), &mut target, Damage::Full);
    r.background().iter().map(|a| a.0).collect()
}

/// **The same history lines drawn before and after the table passes 7/8**,
/// each by a renderer of its own: 800 renditions, then 120 more, 920 named
/// in a 1,024-slot table, so a freeze runs and no eviction would have.
#[test]
fn history_drawn_on_the_cpu_path_keeps_its_colours_past_seven_eighths() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    assert_eq!(t.styles().one().len(), 1024);
    feed(&mut t, &flood(0, 20));
    let before = backgrounds(&mut t, &faces, 10);
    feed(&mut t, &flood(800, 3));
    assert!(t.style_reclaims().rewrites > 0, "the control: it froze");
    let after = backgrounds(&mut t, &faces, 13);
    let wrong = before.iter().zip(&after).filter(|(a, b)| a != b).count();
    assert_eq!(before.len(), 400);
    assert_eq!(wrong, 0, "history cells drawn in another colour");
}

/// One frame's pixels, `back` lines into the history.
fn frame(r: &mut Renderer<'_, '_>, c: &mut Canvas, t: &mut Term<'_>, back: Rows, d: Damage) -> u64 {
    {
        let mut target = c.target();
        let _ = r.render_at(t, back, &mut target, d);
    }
    c.digest()
}

/// **A window scrolled back over frozen history, frame after frame, by one
/// renderer drawing only the rows it finds changed**, is the window a
/// terminal whose table never fills draws: foreground, background and the
/// bold face. Every line holds the same letters in other renditions, so a
/// scroll of one line leaves a lent copy whose ids are the last frame's;
/// the lending's own number is what tells the two frames apart.
#[test]
fn a_window_over_frozen_history_draws_as_a_table_that_never_fills_does() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut small_owned = Owned::new(&WS);
    let mut small = Term::new(small_owned.storage(), &WS);
    let mut big_owned = Owned::new(&BIG);
    let mut big = Term::new(big_owned.storage(), &WS);
    assert_eq!(small.styles().one().len(), 1024);
    assert_eq!(big.styles().one().len(), 65_536);
    let (mut r, mut rr) = (renderer(&faces), renderer(&faces));
    let (mut c, mut rc) = (Canvas::for_renderer(&r), Canvas::for_renderer(&rr));
    let bytes = flood_both(0, 30);
    feed(&mut small, &bytes);
    feed(&mut big, &bytes);
    assert!(small.style_reclaims().rewrites > 0, "the control: it froze");
    assert_eq!(big.style_reclaims().rewrites, 0, "the reference froze");
    let mut damage = Damage::Full;
    for back in [0usize, 20, 19, 18, 17, 16, 17, 20, 0, 18] {
        let got = frame(&mut r, &mut c, &mut small, Rows::of_count(back), damage);
        let want = frame(
            &mut rr,
            &mut rc,
            &mut big,
            Rows::of_count(back),
            Damage::Full,
        );
        assert_eq!(got, want, "the window {back} lines back drawn otherwise");
        damage = Damage::Incremental;
    }
}

/// **SAPV 16's hidden vowels, on a line the core froze**: drawn without
/// them, as the reference draws it, since a cell's marks are read through
/// its own rendition (`drawn_marks`).
#[test]
fn a_frozen_line_written_under_sapv_16_draws_without_its_vowels() {
    // The built face, which draws Hebrew and its points.
    let built = std::fs::read(format!(
        "{}/../baud-app/fonts/built/Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&built).unwrap(), Face::parse(&mono).unwrap()];
    let mut small_owned = Owned::new(&WS);
    let mut small = Term::new(small_owned.storage(), &WS);
    let mut big_owned = Owned::new(&BIG);
    let mut big = Term::new(big_owned.storage(), &WS);
    let mut bytes = Vec::new();
    bytes.extend(
        "\x1b[?25l\x1b[16 ]\x1b[38;2;200;10;10m\u{5D1}\u{5BC}\u{5B0}\u{5D1}\u{5BC}\u{5B0}\x1b[0m\x1b[0 ]\r\n"
            .as_bytes(),
    );
    bytes.extend(flood(0, 23));
    feed(&mut small, &bytes);
    feed(&mut big, &bytes);
    assert!(small.style_reclaims().rewrites > 0, "the control: it froze");
    let back = small.looking_back(Rows::of_count(10_000));
    assert_eq!(back, big.looking_back(Rows::of_count(10_000)));
    let (mut r, mut rr) = (renderer(&faces), renderer(&faces));
    let (mut c, mut rc) = (Canvas::for_renderer(&r), Canvas::for_renderer(&rr));
    let got = frame(&mut r, &mut c, &mut small, back, Damage::Full);
    let want = frame(&mut rr, &mut rc, &mut big, back, Damage::Full);
    assert_eq!(got, want, "the frozen line drawn otherwise");
}
