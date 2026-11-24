//! The hollow cursor, on the CPU path: `[cursor] unfocused_hollow`, drawn
//! while another window has the keyboard. The pixel oracle holds the
//! instance arm and the cell shader to the same four solids, so a rule
//! all three lost would pass it; these hold the CPU path to the rule.
//!
//! Every shape is drawn as a box round its cell, as kitty, ghostty, foot,
//! alacritty and WezTerm draw an unfocused cursor (note AZ), each side the
//! bar's line thick, in the bar's colour, and the cell's own colours left
//! as they are.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::{Blend, CursorLook, Damage, PxSize, Renderer, Stack, Target, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_col: 6,
    ws_row: 2,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// Where the frames were drawn: the frame's width, the cell and the
/// cursor's line.
struct Scene {
    w: Px,
    cw: Px,
    ch: Px,
    line: Px,
}

/// Each of `looks` drawn in turn on one renderer, incrementally as the app
/// draws, with `cursor` the DECSCUSR and `show` whether the cursor is
/// shown: every frame, and the geometry.
fn frames(
    cell: &[u8],
    bytes: &[u8],
    cursor: &[u8],
    looks: &[(CursorLook, bool)],
) -> (Vec<Vec<u32>>, Scene) {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(28.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(6),
        Rows::of_index(2),
    )
    .unwrap();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[1;2H");
    feed(&mut t, cell);
    feed(&mut t, bytes);
    feed(&mut t, cursor);
    feed(&mut t, b"\x1b[1;2H");
    let (w, h) = r.pixel_size();
    let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    let m = r.metrics();
    let mut out = Vec::new();
    for &(look, show) in looks {
        feed(&mut t, if show { b"\x1b[?25h" } else { b"\x1b[?25l" });
        r.set_cursor_look(look);
        {
            let mut target = Target::new(&mut px, w, w, h).unwrap();
            let seal = t.seal_writes();
            r.render(&t, seal, &mut target, Damage::Incremental);
        }
        out.push(px.clone());
    }
    (
        out,
        Scene {
            w,
            cw: m.cell_w,
            ch: m.cell_h,
            line: Px::of(m.thickness.value().max(2)),
        },
    )
}

impl Scene {
    fn at(&self, f: &[u32], x: u32, y: u32) -> u32 {
        f[usize::try_from(y * self.w.value() + x).unwrap()]
    }
    /// Whether `(x, y)` is on the box round column 1, row 0.
    fn on_ring(&self, x: u32, y: u32) -> bool {
        let (x0, x1) = (self.cw.value(), 2 * self.cw.value());
        (x0..x1).contains(&x)
            && y < self.ch.value()
            && (y < self.line.value()
                || y >= self.ch.value() - self.line.value()
                || x < x0 + self.line.value()
                || x >= x1 - self.line.value())
    }
}

const STYLES: [(&str, &[u8]); 3] = [
    ("block", b"\x1b[2 q"),
    ("underline", b"\x1b[4 q"),
    ("bar", b"\x1b[6 q"),
];

/// **Every shape is a box round its cell, in the cursor's colour, with the
/// cell's own colours inside it**: the box's pixels are baud's amber,
/// and every pixel off it, the glyph's included, is the frame without a
/// cursor.
#[test]
fn every_shape_is_a_box_round_its_cell() {
    for (what, style) in STYLES {
        let (f, s) = frames(
            "\u{2588}".as_bytes(),
            b"",
            style,
            &[(CursorLook::Hollow, true), (CursorLook::Hollow, false)],
        );
        let (hollow, none) = (&f[0], &f[1]);
        let fg = baud_render::BaudCursor::dark().fill.0;
        let (mut ring, mut off) = (0, 0);
        for y in 0..2 * s.ch.value() {
            for x in 0..s.w.value() {
                if s.on_ring(x, y) {
                    assert_eq!(s.at(hollow, x, y), fg, "{what}: ({x}, {y}) on the box");
                    ring += 1;
                } else {
                    assert_eq!(
                        s.at(hollow, x, y),
                        s.at(none, x, y),
                        "{what}: ({x}, {y}) off the box"
                    );
                    off += 1;
                }
            }
        }
        assert!(ring > 0 && off > 0, "{what}: the probe reached both");
        // The positive control: the cell's middle is the block's text,
        // which a swap would have turned to the page.
        let mid = s.at(hollow, s.cw.value() + s.cw.value() / 2, s.ch.value() / 2);
        let text = Theme::dark().fg.0 | 0xFF00_0000;
        assert_eq!(mid, text, "{what}: the glyph inside is drawn as it is");
    }
}

/// **Each side is the bar's line thick**: `max(thickness, 2)` pixels, and
/// on a blank cell the pixel inside that is the page.
#[test]
fn each_side_is_the_bars_line_thick() {
    let (f, s) = frames(
        b" ",
        b"",
        b"\x1b[6 q",
        &[(CursorLook::Hollow, true), (CursorLook::Hollow, false)],
    );
    let fg = baud_render::BaudCursor::dark().fill.0;
    let x = s.cw.value() + s.cw.value() / 2;
    assert_eq!(s.at(&f[0], x, s.line.value() - 1), fg, "the top's last row");
    assert_eq!(
        s.at(&f[0], x, s.line.value()),
        s.at(&f[1], x, s.line.value()),
        "the row below it is the page"
    );
    let y = s.ch.value() / 2;
    assert_eq!(
        s.at(&f[0], 2 * s.cw.value() - s.line.value(), y),
        fg,
        "the right side's first column"
    );
    assert_eq!(
        s.at(&f[0], 2 * s.cw.value() - s.line.value() - 1, y),
        s.at(&f[1], 2 * s.cw.value() - s.line.value() - 1, y),
        "the column left of it is the page"
    );
}

/// **A cursor colour draws the box**: `OSC 12`'s, as it draws a bar.
#[test]
fn a_cursor_colour_draws_the_box() {
    let (f, s) = frames(
        b"x",
        b"\x1b]12;#20c040\x07",
        b"\x1b[2 q",
        &[(CursorLook::Hollow, true)],
    );
    assert_eq!(
        s.at(&f[0], s.cw.value() + s.cw.value() / 2, 0),
        0xFF20_C040,
        "the top"
    );
    assert_eq!(
        s.at(&f[0], s.cw.value(), s.ch.value() / 2),
        0xFF20_C040,
        "the left side"
    );
    // Inside the box the page stays: a block would have filled it.
    assert_ne!(
        s.at(&f[0], s.cw.value() + s.line.value(), s.line.value()),
        0xFF20_C040,
        "the page inside the box"
    );
}

/// **A flip of the look alone repaints the cursor's row**: drawn
/// incrementally, shaped then hollow then shaped again, each frame the one
/// a full draw of its look gives.
#[test]
fn a_flip_of_the_look_alone_repaints_the_cursor() {
    for (what, style) in STYLES {
        let (f, _) = frames(
            b"x",
            b"",
            style,
            &[
                (CursorLook::Shaped, true),
                (CursorLook::Hollow, true),
                (CursorLook::Shaped, true),
            ],
        );
        assert_ne!(f[1], f[0], "{what}: hollow is drawn");
        assert_eq!(f[2], f[0], "{what}: shaped again");
        let (full, _) = frames(b"x", b"", style, &[(CursorLook::Hollow, true)]);
        assert_eq!(
            f[1], full[0],
            "{what}: the flip draws what a fresh frame does"
        );
    }
}
