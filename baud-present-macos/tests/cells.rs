//! The two GPU arms draw the same pixels where the cell path's program is
//! the whole program: text a cell at a time, colours, attributes, the
//! cursor in each shape. A pixel that differs is a rule one arm applies
//! and the other does not.
//!
//! macOS only, and it needs a Metal device: it panics without one rather
//! than passing, because a comparison that skips itself reads as a pass.
#![cfg(target_os = "macos")]

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_present_macos::{CellInput, Gpu};
use baud_render::{Blend, Damage, GlyphMap, PxSize, Renderer, Stack, Theme};

const WS: WinSize = WinSize {
    ws_col: 40,
    ws_row: 6,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// A screen with every rule the cell path implements on it, and none it
/// does not: no ligature pair, no combining mark.
fn scene(t: &mut Term<'_>) {
    feed(
        t,
        b"Hello, world. 0123456789 abc XYZ (x) [y] {z}\r\n\
          \x1b[31mred \x1b[1;32mbold green\x1b[0m \x1b[4munderline\x1b[0m \x1b[9mstrike\x1b[0m\r\n\
          \x1b[44m blue bg \x1b[7m reverse \x1b[0m \x1b[2mdim\x1b[0m \x1b[8mhidden\x1b[0m done\r\n\
          \x1b[38;2;200;100;50mtrue colour\x1b[0m \x1b[48;5;27;38;5;226mindexed\x1b[0m \x1b[58;5;196;4mul red\x1b[0m\r\n\
          box \xe2\x94\x8c\xe2\x94\x80\xe2\x94\x90 braille \xe2\xa3\xbf end",
    );
}

struct Arms<'f, 'a> {
    gpu: Gpu,
    renderer: Renderer<'f, 'a>,
}

impl Arms<'_, '_> {
    /// Both arms drawn offscreen and read back.
    fn draw(&mut self, t: &mut Term<'_>) -> (Vec<u32>, Vec<u32>) {
        let (w, h) = self.renderer.pixel_size();
        let n = usize::try_from(w.value() * h.value()).unwrap();
        let target_a = self.gpu.offscreen(w, h).expect("target");
        let target_b = self.gpu.offscreen(w, h).expect("target");
        self.renderer.convert(t, Damage::Full);
        self.gpu.draw(&self.renderer, &target_a).expect("draw");
        let mut a = vec![0u32; n];
        self.gpu.read(&target_a, &mut a).expect("read");
        let cells = t.viewport().to_vec();
        let mut map = GlyphMap::new();
        map.ensure(&cells, &mut self.renderer);
        let cursor = t.cursor_visible().then(|| {
            let (r, c) = t.cursor();
            (r, c, t.cursor_style())
        });
        let styles = self.renderer.resolve_styles(t).to_vec();
        let input = CellInput {
            cells: &cells,
            styles: &styles,
            map: &map,
            cursor,
        };
        self.gpu
            .draw_cells(&self.renderer, &input, &target_b)
            .expect("draw_cells");
        let mut b = vec![0u32; n];
        self.gpu.read(&target_b, &mut b).expect("read");
        assert!(
            map.stats().live.value() > 20,
            "the map holds the scene's glyphs"
        );
        (a, b)
    }
}

fn differing(a: &[u32], b: &[u32]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}

#[test]
fn the_cell_arm_draws_what_the_instance_arm_draws() {
    let data = std::fs::read(format!(
        "{}/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("face");
    let faces = [Face::parse(&data).expect("parse")];
    let renderer = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::from_stride(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut arms = Arms {
        gpu: Gpu::new().expect("a Metal device; this test cannot skip itself"),
        renderer,
    };
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    scene(&mut t);
    // Every cursor shape, block first.
    for (shape, name) in [
        (b"\x1b[2 q", "block"),
        (b"\x1b[4 q", "underline"),
        (b"\x1b[6 q", "bar"),
    ] {
        feed(&mut t, shape);
        let (a, b) = arms.draw(&mut t);
        let drawn = a.iter().filter(|&&p| p != 0).count();
        assert!(
            drawn > a.len() / 2,
            "the instance arm drew the scene, {name}"
        );
        let diff = differing(&a, &b);
        assert_eq!(
            diff, 0,
            "{diff} pixels differ between the arms with a {name} cursor"
        );
    }
    // The positive control: moving the cursor makes the arms disagree, so
    // an equal pair above is two identical drawings and not two blanks.
    let (a, _) = arms.draw(&mut t);
    feed(&mut t, b"\x1b[1;1H");
    let (_, b) = arms.draw(&mut t);
    assert!(differing(&a, &b) > 0, "the comparison can fail");
}
