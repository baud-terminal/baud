//! **The kept frame's cursor row is repainted where the grid is**: with
//! the wrap marks' margins lent, [`Renderer::render_hidden`] paints the
//! grid's columns past them, so the kept frame equals the screen drawn
//! with no cursor, as it does with no margins.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::{Blend, Damage, Leaving, PxSize, Renderer, Stack, Target, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 6,
    ws_col: 20,
};

fn feed(t: &mut Term<'_>, bytes: &str) {
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn renderer<'f, 'a>(faces: &'f [Face<'a>], band: Px) -> Renderer<'f, 'a> {
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
    r
}

fn paint(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, px: &mut [u32], d: Damage) {
    r.fit(t);
    r.place_marks(t, Rows::ZERO, Leaving::None, Px::of(0));
    let (w, h) = r.whole_size();
    let mut target = Target::new(px, w, w, h).unwrap();
    r.render(t, &mut target, d);
}

fn check(band: Px) -> Vec<usize> {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        "\x1b[1 q$ ls\r\nbaud-app docs\r\n$ vim main.rs\x1b[3;3H",
    );
    let mut r = renderer(&faces, band);
    let (w, h) = r.whole_size();
    let width = usize::try_from(w.value()).expect("a width");
    let height = usize::try_from(h.value()).expect("a height");
    // The screen with no cursor, drawn whole by a renderer of its own.
    feed(&mut t, "\x1b[?25l");
    let mut fresh = renderer(&faces, band);
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
    assert_eq!(check(Px::of(0)), Vec::<usize>::new());
}

#[test]
fn render_hidden_with_margins_is_the_screen_with_no_cursor() {
    assert_eq!(check(Px::of(16)), Vec::<usize>::new());
}
