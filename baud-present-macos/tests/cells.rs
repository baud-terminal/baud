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
use baud_domain::{BytePos, Col, Cols, Depth, PackedCell, ReadLen, Row, RowRot, Rows, WinSize};
use baud_font::Face;
use baud_present_macos::{
    CellInput, Done, Drawn, Gpu, GridBuffer, MapBuffer, OverlayBuffer, StylesBuffer,
};
use baud_render::{
    Blend, Candidate, Damage, GlyphMap, Highlight, Instance, Overlay, PxSize, Renderer, Resolved,
    Scene, Stack, Theme, Window,
};

/// **Wide enough for the right-hand side to be a question.** It was 40x6,
/// and a scene that never reaches a far column cannot show a path that
/// stops short of one -- which is what the owner reported asciiquarium
/// doing, with its waterline ending two thirds across a window whose
/// cells the core had filled to the last column.
const WS: WinSize = WinSize {
    ws_col: 120,
    ws_row: 10,
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
          arrows a -> b => c != d <= e // f :: g <- end\r\n",
    );
    // A band that runs to the very last column, and one that stops a cell
    // short of it: asciiquarium's waterline is exactly this shape, and
    // between them they separate "the last column is not drawn" from "the
    // right-hand side is not drawn".
    let full = "~".repeat(usize::from(WS.ws_col));
    let short = "^".repeat(usize::from(WS.ws_col) - 1);
    feed(t, format!("\x1b[36m{full}\x1b[0m").as_bytes());
    feed(t, format!("\r\n\x1b[36m{short}\x1b[0m").as_bytes());
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
    /// Where the window sits, for the scrollbar both arms draw.
    window: Option<Window>,
    /// The window copied flat, in screen order -- research decision D4's
    /// buffer, and what the app binds. Held here so a third arm can draw
    /// from it and be compared against the two that read the live grid.
    copy: GridBuffer,
    copy_mem: &'static mut [PackedCell],
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
        self.renderer.set_scrollbar(self.window);
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
        self.records.extend_from_slice(self.renderer.scrollbar());
        let input = CellInput {
            set: baud_present_macos::journal::SetIx(0),
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

    /// The cell path again, reading **a copy of the window** rather than
    /// the core's grid: D4's buffer, laid out flat in screen order, so the
    /// shader's rotation is the identity and its ring is the screen.
    ///
    /// Everything else is the frame `draw` just built -- the same styles,
    /// the same map, the same overlay and records -- so a difference in
    /// the pixels can only be the grid it read.
    fn draw_copied(&mut self, t: &mut Term<'_>) -> Vec<u32> {
        let (w, h) = self.renderer.pixel_size();
        let n = usize::try_from(w.value() * h.value()).unwrap();
        let target = self.gpu.offscreen(w, h).expect("target");
        let view = t.view(self.back);
        let stride = t.stride().index();
        let (cols, rows) = (view.cols().index(), view.rows().index());
        for y in 0..rows {
            let src = view.row(Row::from_index(y));
            let at = y * stride;
            self.copy_mem[at..at + cols].copy_from_slice(&src[..cols]);
        }
        let cursor = (t.cursor_visible() && self.back.index() == 0).then(|| {
            let (r, c) = t.cursor();
            (r, c, t.cursor_style())
        });
        let input = CellInput {
            set: baud_present_macos::journal::SetIx(0),
            grid: &self.copy,
            styles: &self.styles,
            map: &self.map_buffer,
            stride: t.stride(),
            rotation: RowRot::NONE,
            ring: Rows::of_index(rows),
            cursor,
            highlight: self.highlight,
            overlay: &self.overlay,
            records: &self.records,
        };
        let Some(Drawn::Frame(_)) =
            self.gpu
                .draw_cells(&mut self.renderer, &input, &target, Done::Wait)
        else {
            panic!("draw_cells over the copy");
        };
        let mut out = vec![0u32; n];
        self.gpu.read(&target, &mut out).expect("read");
        out
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
    // **Translucent on purpose.** At `Theme::dark()`'s `0xFF` the window
    // dim is a no-op, so the two arms would agree about a rule neither
    // executes -- and the swap rule this compares (an opaque foreground
    // handed back by a cursor or a selection) is only visible when a
    // background carries an alpha below one.
    let theme = Theme {
        bg_alpha: baud_domain::Chan::from_u8(56),
        ..Theme::dark()
    };
    let renderer = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        theme,
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
    let owned_stride = owned.stride();
    let mut t = Term::new(owned.storage_over(cells, alt), &WS);
    let (styles, styles_mem) = gpu.styles(&t).expect("shared styles");
    let (map_buffer, map_mem) = gpu.map().expect("a shared map");
    let (overlay, overlay_mem) = gpu
        .overlay(
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .expect("a shared overlay");
    // The window's own buffer: the screen's height at the core's pitch,
    // which is what the app provisions.
    let (copy, copy_mem) = gpu
        .grid(Rows::of_index(usize::from(WS.ws_row)), owned_stride)
        .expect("a shared window copy");
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
        window: None,
        copy,
        copy_mem,
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
        let (pw, _) = arms.renderer.pixel_size();
        let m = arms.renderer.metrics();
        let boxed = diff_box(
            &a,
            &b,
            usize::try_from(pw.value()).unwrap(),
            (
                usize::try_from(m.cell_w.value()).unwrap(),
                usize::try_from(m.cell_h.value()).unwrap(),
            ),
        );
        if diff > 0 {
            // Which arm is blank there decides everything: the instance
            // path is the oracle, so if the cell arm is empty the cells
            // were masked by the overlay and nothing drew them back.
            let (cw, ch) = (
                usize::try_from(m.cell_w.value()).unwrap(),
                usize::try_from(m.cell_h.value()).unwrap(),
            );
            let w = usize::try_from(pw.value()).unwrap();
            if let Some((x0, y0, x1, y1)) = boxed {
                let mut lit = (0usize, 0usize);
                for py in y0 * ch..(y1 + 1) * ch {
                    for px in x0 * cw..(x1 + 1) * cw {
                        let i = py * w + px;
                        lit.0 += usize::from(a.get(i).is_some_and(|&p| p != 0));
                        lit.1 += usize::from(b.get(i).is_some_and(|&p| p != 0));
                    }
                }
                eprintln!(
                    "differing region: instance arm {} lit pixels, cell arm {} lit",
                    lit.0, lit.1
                );
            }
        }
        assert_eq!(
            diff, 0,
            "{diff} pixels differ between the arms with a {name} cursor, \
             cells {boxed:?} of {}x{}",
            WS.ws_col, WS.ws_row
        );
        // **D4's arm: the same frame read out of a copy of the window.**
        // The app binds this buffer rather than the core's grid, so that
        // the parser may keep feeding while the GPU reads. If the copy
        // and the live grid ever drew different pixels, every frame the
        // app shows would be wrong in a way no other gate here can see.
        let c = arms.draw_copied(&mut t);
        let diff = differing(&b, &c);
        assert_eq!(
            diff, 0,
            "{diff} pixels differ between the live grid and its copy, {name} cursor"
        );
    }
    // Scroll the scene so the grid is under a rotation, and compare again:
    // the cell arm reads it rotated, the instance arm turns it.
    // From the last row, so the two newlines scroll whatever the grid's
    // height is -- it was six rows when this was written and is ten now.
    feed(
        &mut t,
        format!("\x1b[{};1H\r\nafter one\r\nafter two", WS.ws_row).as_bytes(),
    );
    assert_ne!(t.rotation().index(), 0, "the scene scrolled");
    let (a, b) = arms.draw(&mut t);
    let diff = differing(&a, &b);
    assert_eq!(
        diff,
        0,
        "{diff} pixels differ under a rotation of {}",
        t.rotation().index()
    );
    // The rotation is the case the copy has to resolve for itself: it is
    // flat, so a wrong walk would show as rows in the wrong order rather
    // than as anything subtle.
    let c = arms.draw_copied(&mut t);
    assert_eq!(
        differing(&b, &c),
        0,
        "the copy disagrees under a rotation of {}",
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
    // And three lines up the ring, where the window wraps rows the copy
    // has to gather from two places.
    let c = arms.draw_copied(&mut t);
    assert_eq!(differing(&b, &c), 0, "the copy disagrees three lines back");
    assert!(
        differing(&c, &live) > 0,
        "the copy three lines back is not the live screen"
    );
    // The scrollbar's thumb, three lines up a ring with history behind
    // it: one solid on the instance list and on the cell path's records,
    // equal between the arms and a change to the pixels.
    arms.window = Some(Window {
        retained: Rows::of_count(t.scrollback().len()),
        back: Rows::of_count(3),
        rows: t.viewport().rows(),
    });
    let (bar_a, bar_b) = arms.draw(&mut t);
    assert_eq!(
        differing(&bar_a, &bar_b),
        0,
        "the arms disagree with a scrollbar"
    );
    assert!(differing(&bar_a, &a) > 0, "the thumb reached the pixels");
    arms.window = None;
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
