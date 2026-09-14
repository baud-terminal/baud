//! The two GPU arms draw the same pixels where the cell path's program is
//! the whole program: text a cell at a time, colours, attributes, the
//! cursor in each shape. A pixel that differs is a rule one arm applies
//! and the other does not.
//!
//! The cell arm reads the terminal's viewport in place -- the grid is
//! shared GPU memory lent to the core, under its rotation -- so the test
//! also checks that a grid read that way, rotation and all, matches the
//! instance arm's turned copy.
//!
//! macOS only, and it needs a Metal device: it panics without one rather
//! than passing, because a comparison that skips itself reads as a pass.
#![cfg(target_os = "macos")]

use baud_core::{Owned, Term};
use baud_domain::CacheGeometry;
use baud_domain::{BytePos, Col, Cols, Depth, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_present_macos::{
    CellInput, Done, Drawn, Gpu, GridBuffer, MapBuffer, OverlayBuffer, StylesBuffer,
};
use baud_render::{
    Blend, Candidate, Damage, GlyphMap, Highlight, Instance, Overlay, PxSize, Renderer, Resolved,
    Scene, Stack, Theme,
};

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
          box \xe2\x94\x8c\xe2\x94\x80\xe2\x94\x90 braille \xe2\xa3\xbf end\r\n\
          marks e\xcc\x81 a\xcc\x88 o\xcc\x82 composed\r\n\
          arrows a -> b => c != d <= e // f :: g <- end",
    );
}

struct Arms<'f, 'a> {
    gpu: Gpu,
    renderer: Renderer<'f, 'a>,
    grid: GridBuffer,
    styles: StylesBuffer,
    styles_mem: &'static mut [Resolved],
    map_buffer: MapBuffer,
    map: GlyphMap<'static>,
    overlay: OverlayBuffer,
    overlay_mem: &'static mut [u8],
    exceptions: Overlay,
    records: Vec<Instance>,
    candidates: Vec<Candidate>,
    /// Lines above the live screen both arms draw.
    back: Rows,
    /// The selection both arms draw, or none.
    highlight: Option<Highlight>,
}

impl Arms<'_, '_> {
    /// Both arms drawn offscreen and read back.
    fn draw(&mut self, t: &mut Term<'_>) -> (Vec<u32>, Vec<u32>) {
        let (w, h) = self.renderer.pixel_size();
        let n = usize::try_from(w.value() * h.value()).unwrap();
        let target_a = self.gpu.offscreen(w, h).expect("target");
        let target_b = self.gpu.offscreen(w, h).expect("target");
        // The cell arm first: it reads the grid under its rotation, and the
        // instance arm's `convert` would turn it in place.
        let view = t.view(self.back);
        self.renderer.set_highlight(self.highlight);
        self.map
            .ensure(view, &mut self.renderer, &mut self.candidates);
        let cursor = (t.cursor_visible() && self.back.index() == 0).then(|| {
            let (r, c) = t.cursor();
            (r, c, t.cursor_style())
        });
        self.renderer.resolve_styles_into(t, self.styles_mem);
        self.exceptions.ensure(
            &Scene {
                term: t,
                view,
                styles: self.styles_mem,
                cursor,
                highlight: self.highlight,
                map: &self.map,
                candidates: &self.candidates,
            },
            &mut self.renderer,
            self.overlay_mem,
            &mut self.records,
        );
        let input = CellInput {
            grid: &self.grid,
            styles: &self.styles,
            map: &self.map_buffer,
            stride: t.stride(),
            rotation: view.rotation(),
            ring: t.ring(),
            cursor,
            highlight: self.highlight,
            overlay: &self.overlay,
            records: &self.records,
        };
        let Some(Drawn::Frame(_)) =
            self.gpu
                .draw_cells(&mut self.renderer, &input, &target_b, Done::Wait)
        else {
            panic!("draw_cells");
        };
        let mut b = vec![0u32; n];
        self.gpu.read(&target_b, &mut b).expect("read");
        self.renderer.convert(t, self.back, Damage::Full);
        let Some(Drawn::Frame(_)) = self.gpu.draw(&mut self.renderer, &target_a, Done::Wait) else {
            panic!("draw");
        };
        let mut a = vec![0u32; n];
        self.gpu.read(&target_a, &mut a).expect("read");
        assert!(
            self.map.stats().live.value() > 20,
            "the map holds the scene's glyphs"
        );
        (a, b)
    }
}

fn differing(a: &[u32], b: &[u32]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}

/// The bounding box of the differing pixels, in cells: (col0, row0, col1, row1).
fn diff_box(
    a: &[u32],
    b: &[u32],
    w: usize,
    cell: (usize, usize),
) -> Option<(usize, usize, usize, usize)> {
    let mut lo = (usize::MAX, usize::MAX);
    let mut hi = (0, 0);
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        if x != y {
            let (px, py) = (i % w, i / w);
            lo = (lo.0.min(px), lo.1.min(py));
            hi = (hi.0.max(px), hi.1.max(py));
        }
    }
    (lo.0 != usize::MAX).then(|| (lo.0 / cell.0, lo.1 / cell.1, hi.0 / cell.0, hi.1 / cell.1))
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
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let gpu = Gpu::new().expect("a Metal device; this test cannot skip itself");
    // The viewport is the GPU's memory: the core writes it, the shader
    // reads it.
    let mut owned = Owned::without_surfaces(&WS, &CacheGeometry::UNKNOWN, Depth::of(12));
    let (grid, cells) = gpu
        .grid(owned.ring(), owned.stride())
        .expect("a shared grid");
    let (_, alt) = gpu
        .grid(owned.rows(), owned.stride())
        .expect("a shared alternate");
    let mut t = Term::new(owned.storage_over(cells, alt), &WS);
    let (styles, styles_mem) = gpu.styles(&t).expect("shared styles");
    let (map_buffer, map_mem) = gpu.map().expect("a shared map");
    let (overlay, overlay_mem) = gpu
        .overlay(
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .expect("a shared overlay");
    let mut arms = Arms {
        gpu,
        renderer,
        grid,
        styles,
        styles_mem,
        map_buffer,
        map: GlyphMap::over(map_mem),
        overlay,
        overlay_mem,
        exceptions: Overlay::new(),
        records: Vec::new(),
        candidates: Vec::new(),
        back: Rows::ZERO,
        highlight: None,
    };
    scene(&mut t);
    // Every cursor shape, block first.
    for (shape, name) in [
        (b"\x1b[2 q", "block"),
        (b"\x1b[4 q", "underline"),
        (b"\x1b[6 q", "bar"),
    ] {
        feed(&mut t, shape);
        let (a, b) = arms.draw(&mut t);
        // Three marked cells, and seven ligature pairs under JetBrains
        // Mono, each shaped in a window and drawn by at least one record.
        eprintln!("overlay {name}: {:?}", arms.exceptions.stats());
        assert!(
            arms.exceptions.stats().cells.value() >= 3 + 7,
            "the marked and ligature cells went to the records: {:?}",
            arms.exceptions.stats()
        );
        assert!(
            arms.exceptions.stats().windows.value() >= 1,
            "the admitted pairs were shaped in windows: {:?}",
            arms.exceptions.stats()
        );
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
    // Scroll the scene so the grid is under a rotation, and compare again:
    // the cell arm reads it rotated, the instance arm turns it.
    feed(&mut t, b"\x1b[6;1H\r\nafter one\r\nafter two");
    assert_ne!(t.rotation().index(), 0, "the scene scrolled");
    let (a, b) = arms.draw(&mut t);
    let diff = differing(&a, &b);
    assert_eq!(
        diff,
        0,
        "{diff} pixels differ under a rotation of {}",
        t.rotation().index()
    );
    // Looking back into the history: both arms draw the window three lines
    // up the ring, the cell arm by a retreated rotation and the instance
    // arm by the same view, and they agree -- and differ from the live
    // screen, so the offset reached the pixels.
    let (live, _) = arms.draw(&mut t);
    arms.back = Rows::of_count(3);
    let (a, b) = arms.draw(&mut t);
    assert_eq!(differing(&a, &b), 0, "the arms disagree three lines back");
    assert!(
        differing(&a, &live) > 0,
        "three lines back is not the live screen"
    );
    arms.back = Rows::ZERO;
    // A selection over the marked and ligature rows, with the block cursor
    // inside it: both arms swap the selected cells' colours and the cursor
    // swaps again, on the cell passes and on the records alike -- and the
    // selection reaches the pixels.
    feed(&mut t, b"\x1b[2 q\x1b[2;3H");
    let (plain, _) = arms.draw(&mut t);
    arms.highlight = Some(Highlight {
        from: (Row::from_index(1), Col::at_index(2)),
        to: (Row::from_index(4), Col::at_index(30)),
    });
    let (a, b) = arms.draw(&mut t);
    {
        let (w, _) = arms.renderer.pixel_size();
        let m = arms.renderer.metrics();
        let cell = (
            usize::try_from(m.cell_w.value()).unwrap(),
            usize::try_from(m.cell_h.value()).unwrap(),
        );
        let bx = diff_box(&a, &b, usize::try_from(w.value()).unwrap(), cell);
        assert_eq!(
            differing(&a, &b),
            0,
            "the arms disagree under a selection, in cells {bx:?}"
        );
    }
    assert!(
        differing(&a, &plain) > 0,
        "the selection changed the pixels"
    );
    arms.highlight = None;
    // The positive control: moving the cursor makes the arms disagree, so
    // an equal pair above is two identical drawings and not two blanks.
    let (a, _) = arms.draw(&mut t);
    feed(&mut t, b"\x1b[1;1H");
    let (_, b) = arms.draw(&mut t);
    assert!(differing(&a, &b) > 0, "the comparison can fail");
}
