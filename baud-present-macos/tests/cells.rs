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

use baud_core::{DynamicColor, Owned, Term};
use baud_domain::CacheGeometry;
use baud_domain::{
    BytePos, Col, Cols, Depth, ImageBudget, PackedCell, ReadLen, Row, RowRot, Rows, WinSize,
};
use baud_font::{Face, Px};
use baud_present_macos::{
    CellInput, Done, Drawn, Gpu, GridBuffer, MapBuffer, OverlayBuffer, StylesBuffer,
};
use baud_render::{
    Blend, Candidate, Damage, GlyphMap, Highlight, Instance, Overlay, PxSize, Region, Renderer,
    Resolved, Scene, Stack, Theme, Trail, Transition, Window,
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

/// A real 64x48 ImageMagick Sixel -- the same capture the domain and CPU
/// gates decode. Placed at the top-left of the scene, over the text, so both
/// GPU arms must draw it identically.
const SWIRL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tests/recordings/sixel/swirl.raw"
));

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// A screen with every rule the cell path implements on it, and none it
/// does not: no ligature pair, no combining mark. The second box row is
/// the procedural range -- heavy, double, arc, diagonal, dashed, block,
/// shade and braille, and a red heavy corner -- which both arms must take
/// from baud's own masks rather than the face. The first box row ends in
/// what a text-drawn image is made of -- block elements, a space and a
/// box-drawing stroke on a set background, and a reversed block -- which
/// the theme's `AppBg::Opaque` keeps opaque, as it does the fourth row's
/// `blue bg` and `reverse` spans.
fn scene(t: &mut Term<'_>) {
    feed(
        t,
        b"Hello, world. 0123456789 abc XYZ (x) [y] {z}\r\n\
          \x1b[31mred \x1b[1;32mbold green\x1b[0m \x1b[4munderline\x1b[0m \x1b[9mstrike\x1b[0m\r\n\
          \x1b[4:3mcurly\x1b[0m \x1b[4:4mdotted\x1b[0m \x1b[4:5mdashed\x1b[0m \x1b[58;5;196;4:3mred curl\x1b[0m\r\n\
          \x1b[44m blue bg \x1b[7m reverse \x1b[0m \x1b[2mdim\x1b[0m \x1b[8mhidden\x1b[0m done\r\n\
          \x1b[38;2;200;100;50mtrue colour\x1b[0m \x1b[48;5;27;38;5;226mindexed\x1b[0m \x1b[58;5;196;4mul red\x1b[0m\r\n\
          box \xe2\x94\x8c\xe2\x94\x80\xe2\x94\x90 braille \xe2\xa3\xbf end \x1b[38;2;200;0;0;48;2;0;0;200m\xe2\x96\x80\xe2\x96\x80 \xe2\x94\x80\x1b[0;7;32m\xe2\x96\x84\x1b[0m image\r\n\
          \xe2\x94\x8f\xe2\x94\x81\xe2\x94\xb3\xe2\x94\x93 \xe2\x95\x94\xe2\x95\x90\xe2\x95\xa6\xe2\x95\x97 \xe2\x95\xad\xe2\x94\x80\xe2\x95\xae \xe2\x95\xb1\xe2\x95\xb2\xe2\x95\xb3 \xe2\x94\x84\xe2\x94\x8c\xe2\x95\x8c \xe2\x96\x80\xe2\x96\x84\xe2\x96\x88\xe2\x96\x8c\xe2\x96\x90\xe2\x96\x91\xe2\x96\x92\xe2\x96\x93\xe2\x96\x9a\xe2\x96\x9f \xe2\xa0\x81\xe2\xa0\xbf\xe2\xa3\x80\xe2\xa2\xb2 \x1b[31m\xe2\x94\x8f\xe2\x94\x81\xe2\x94\x93\x1b[0m\r\n\
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
    /// The cursor trail both arms draw, or none.
    trail: Option<Trail>,
    /// The alternate-screen fade both arms draw, or none.
    fade: Option<Transition>,
    /// The window copied flat, in screen order -- research decision D4's
    /// buffer, and what the app binds. Held here so a third arm can draw
    /// from it and be compared against the two that read the live grid.
    copy: GridBuffer,
    copy_mem: &'static mut [PackedCell],
    /// Smooth scroll: the pixels the screen is shifted down, or none. Both
    /// cell arms draw one extra leaving row above the window when set.
    slide: Option<Px>,
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
        self.renderer.set_trail(self.trail);
        self.renderer.set_screen_fade(self.fade);
        // Both arms draw the same image placements: computed once here, read by
        // `draw` and `draw_cells` alike.
        self.renderer.set_images(t, self.back);
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
                // Smooth scroll: the records slide with the grid, matching
                // the `slide` handed to the CellInput below, so the marked
                // and ligatured slide gate compares like against like.
                pan: self.slide.unwrap_or(Px::of(0)),
            },
            &mut self.renderer,
            self.overlay_mem,
            &mut self.records,
        );
        self.records.extend_from_slice(self.renderer.trail());
        self.records.extend_from_slice(self.renderer.scrollbar());
        self.records.extend_from_slice(self.renderer.screen_fade());
        let input = CellInput {
            set: baud_present_macos::journal::SetIx(0),
            grid: &self.grid,
            styles: &self.styles,
            map: &self.map_buffer,
            stride: t.stride(),
            rotation: view.rotation(),
            ring: t.ring(),
            cursor,
            cursor_color: t.dynamic_color(DynamicColor::Cursor),
            highlight: self.highlight,
            overlay: &self.overlay,
            records: &self.records,
            // In place: the shared ring holds the leaving row one row up
            // (ring row `rot - 1`), reached by the shader's own modulo.
            slide: self.slide,
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
        self.renderer.set_images(t, self.back);
        let stride = t.stride().index();
        let (cols, rows) = (view.cols().index(), view.rows().index());
        for y in 0..rows {
            let src = view.row(Row::from_index(y));
            let at = y * stride;
            self.copy_mem[at..at + cols].copy_from_slice(&src[..cols]);
        }
        // Smooth scroll: append the leaving row -- the line just above the
        // window, `view(back + 1).row(0)` -- at index `rows`, and read one
        // more row, so the shader's `slide_rows = 1` maps draw row 0 to it
        // with the identity rotation.
        let ring = if self.slide.is_some() {
            let above = t.view(Rows::of_index(self.back.index() + 1));
            let src = above.row(Row::from_index(0));
            let at = rows * stride;
            self.copy_mem[at..at + cols].copy_from_slice(&src[..cols]);
            rows + 1
        } else {
            rows
        };
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
            ring: Rows::of_index(ring),
            cursor,
            cursor_color: t.dynamic_color(DynamicColor::Cursor),
            highlight: self.highlight,
            overlay: &self.overlay,
            records: &self.records,
            slide: self.slide,
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
    // The committed Noto subset behind it, for the one wide character the
    // scene measures: U+231A, which a block's edges cut on both sides.
    let noto = std::fs::read(format!(
        "{}/../baud-font/tests/fonts/NotoColorEmoji-subset.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("face");
    let faces = [
        Face::parse(&data).expect("parse"),
        Face::parse(&noto).expect("parse"),
    ];
    // **Translucent on purpose.** At `Theme::dark()`'s `0xFF` the window
    // dim is a no-op, so the two arms would agree about a rule neither
    // executes -- and the swap rule this compares (an opaque foreground
    // handed back by a cursor or a selection) is only visible when a
    // background carries an alpha below one.
    // And `[glass] app_backgrounds = "opaque"`: every background the scene
    // sets -- the image cells at the end of the `box` row, the `blue bg`
    // and `reverse` spans -- keeps full alpha, while the default background
    // everywhere else stays dimmed, so both halves of the rule are compared
    // across the arms.
    let theme = Theme {
        bg_alpha: baud_domain::Chan::from_u8(56),
        app_bg: baud_render::AppBg::Opaque,
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
    // With image memory: without it a stored Sixel is *refused*, both arms
    // draw nothing, and the image comparison passes while proving the opposite
    // -- the `golden.rs` trap this file exists not to fall into.
    let mut owned = Owned::without_surfaces(&WS, &CacheGeometry::UNKNOWN, Depth::of(12))
        .with_images(ImageBudget::of(64 * 48 + 64, 8192));
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
    // The window's own buffer: the screen's height plus one, so a smooth
    // scroll can append the leaving row, at the core's pitch -- what the
    // app provisions.
    let (copy, copy_mem) = gpu
        .grid(Rows::of_index(usize::from(WS.ws_row) + 1), owned_stride)
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
        trail: None,
        fade: None,
        copy,
        copy_mem,
        slide: None,
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
    // **`OSC 12`: an application-set cursor colour.** Two things are gated.
    // The block fills with it and the bar and underline cursors take it, and
    // the two arms must still agree pixel for pixel -- a dropped `cursor_color`
    // on either arm is the wrong-colour bug the translucent-background work
    // found in three places. And the colour must *move the pixels* off the
    // reverse-video default: without that check a `cursor_color` dropped on
    // *both* arms would pass vacuously, both agreeing on the reverse video --
    // the `SHAPE_SLOTS` near-miss this scene exists to avoid.
    feed(&mut t, b"\x1b[2 q\x1b[1;1H");
    let (plain, _) = arms.draw(&mut t);
    feed(&mut t, b"\x1b]12;rgb:ff/00/00\x07");
    let (block_a, block_b) = arms.draw(&mut t);
    assert_ne!(
        differing(&plain, &block_a),
        0,
        "the OSC 12 colour never reached the block cursor's pixels"
    );
    assert_eq!(
        differing(&block_a, &block_b),
        0,
        "the arms disagree on an OSC 12 block cursor"
    );
    for (shape, name) in [
        (b"\x1b[4 q".as_slice(), "osc12 underline"),
        (b"\x1b[6 q".as_slice(), "osc12 bar"),
    ] {
        feed(&mut t, shape);
        let (a, b) = arms.draw(&mut t);
        assert_eq!(
            differing(&a, &b),
            0,
            "the arms disagree on an {name} cursor"
        );
    }
    // Reset, so the rotation and selection cases below draw the default cursor.
    feed(&mut t, b"\x1b]112\x07");
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
        region: Region::Stream,
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
    // A block, and its edges cut a wide character on each side: U+231A
    // straddles the left edge on row 2 (lead 100, tail 101) and the right
    // edge on row 3 (lead 101, tail 102), so a tail is out on one row and
    // in on the other -- on the cell shader by its own `WIDE_TAIL` test,
    // on the instance path by `Highlight::selects`. The corners, rows 1
    // and 4 at column 101, are blank.
    let watch = "\u{231a}";
    feed(
        &mut t,
        format!("\x1b[3;101H{watch}\x1b[4;102H{watch}\x1b[2;3H").as_bytes(),
    );
    {
        let view = t.viewport();
        assert!(view.row(Row::from_index(2))[101].content.is_wide_tail());
        assert!(view.row(Row::from_index(3))[102].content.is_wide_tail());
    }
    arms.highlight = None;
    let (unselected, _) = arms.draw(&mut t);
    let corners = (
        (Row::from_index(1), Col::at_index(101)),
        (Row::from_index(4), Col::at_index(101)),
    );
    arms.highlight = Some(Highlight {
        from: corners.0,
        to: corners.1,
        region: Region::Block,
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
            "the arms disagree under a block, in cells {bx:?}"
        );
    }
    assert!(
        differing(&a, &unselected) > 0,
        "the block changed the pixels"
    );
    // The same corners as a stream select whole middle rows, which a
    // block does not: the region reaches the pixels.
    arms.highlight = Some(Highlight {
        from: corners.0,
        to: corners.1,
        region: Region::Stream,
    });
    let (stream, _) = arms.draw(&mut t);
    assert!(
        differing(&a, &stream) > 0,
        "a block and a stream over the same corners drew the same"
    );
    arms.highlight = None;
    // The cursor trail: a fading ghost mid-sweep between two cells, drawn
    // by both arms -- one translucent solid on the instance list and on the
    // cell path's records -- and a change to the pixels. `plain` has the
    // same cursor and no trail, so the difference is the ghost alone.
    arms.trail = Some(Trail {
        from: (Row::from_index(1), Col::at_index(2)),
        to: (Row::from_index(4), Col::at_index(20)),
        progress: 0.5,
    });
    let (trail_a, trail_b) = arms.draw(&mut t);
    {
        let (w, _) = arms.renderer.pixel_size();
        let m = arms.renderer.metrics();
        let cell = (
            usize::try_from(m.cell_w.value()).unwrap(),
            usize::try_from(m.cell_h.value()).unwrap(),
        );
        let bx = diff_box(
            &trail_a,
            &trail_b,
            usize::try_from(w.value()).unwrap(),
            cell,
        );
        assert_eq!(
            differing(&trail_a, &trail_b),
            0,
            "the arms disagree under a cursor trail, in cells {bx:?}"
        );
    }
    assert!(
        differing(&trail_a, &plain) > 0,
        "the trail reached the pixels"
    );
    arms.trail = None;
    // The alternate-screen fade: a full-screen veil at mid-progress, drawn
    // by both arms as one translucent solid over everything, and a change to
    // the pixels against the unveiled frame it was drawn over.
    let (unveiled, _) = arms.draw(&mut t);
    arms.fade = Some(Transition { progress: 0.5 });
    let (fade_a, fade_b) = arms.draw(&mut t);
    assert_eq!(
        differing(&fade_a, &fade_b),
        0,
        "the arms disagree under a screen fade"
    );
    assert!(
        differing(&fade_a, &unveiled) > 0,
        "the fade veiled the screen"
    );
    arms.fade = None;
    // The positive control: moving the cursor makes the arms disagree, so
    // an equal pair above is two identical drawings and not two blanks.
    let (a, _) = arms.draw(&mut t);
    feed(&mut t, b"\x1b[1;1H");
    let (_, b) = arms.draw(&mut t);
    assert!(differing(&a, &b) > 0, "the comparison can fail");

    // Smooth scroll. Hide the cursor -- the settled `back = 1` reference
    // below cannot show it and the slid `back = 0` frame would. Plain digit
    // content, cleared first: no combining marks and no ligature pairs, so
    // the overlay is empty and this gates the slide's cell math, not the
    // separate question of sliding the mark and ligature records (which the
    // marked and ligature rows above already gate at rest). Enough lines to
    // scroll history in, so there is a leaving row above the window. Its
    // glyphs are one row up, not in `view(0)`, so ensure the map over
    // `view(1)`, which the app's persistent map holds for free.
    feed(&mut t, b"\x1b[?25l\x1b[2J\x1b[H");
    for i in 0..14u32 {
        feed(
            &mut t,
            format!("0000 1111 2222 row {i:02} 3333 4444 5555\r\n").as_bytes(),
        );
    }
    let cell_h = arms.renderer.metrics().cell_h;
    arms.map.ensure(
        t.view(Rows::of_count(1)),
        &mut arms.renderer,
        &mut arms.candidates,
    );

    // A full-row slide at the live screen is the screen one line up. At
    // `pan = cell_h`, the leaving row fills the top and every row lands on a
    // settled grid line, so the frame is exactly `view(1)` -- the pre-scroll
    // screen. Comparing against the trusted **non-slide** path is the strong
    // gate: a systematic slide error both cell layouts share, which a
    // layout-vs-layout check could not see, moves this against a reference
    // that does not run the slide shader at all.
    arms.back = Rows::ZERO;
    arms.slide = Some(cell_h);
    let (_, full_slid) = arms.draw(&mut t);
    arms.slide = None;
    arms.back = Rows::of_count(1);
    let (_, one_up) = arms.draw(&mut t);
    assert_eq!(
        differing(&full_slid, &one_up),
        0,
        "a full-row slide is not the screen one line up"
    );

    // Mid-slide, where there is no non-slide reference: the two cell layouts
    // -- the shared ring read in place, and the flat window copy the app
    // binds -- must still agree pixel for pixel at a fractional pan, so the
    // app's copy path is sound wherever the slide is.
    arms.back = Rows::ZERO;
    arms.slide = Some(Px::of(cell_h.value() / 2));
    let (_, mid_in_place) = arms.draw(&mut t);
    let mid_copy = arms.draw_copied(&mut t);
    assert_eq!(
        differing(&mid_in_place, &mid_copy),
        0,
        "the two cell layouts disagree mid-slide"
    );
    // And the slide reached the pixels: a mid-slide frame is not the settled
    // one, so the pan is not being dropped.
    arms.slide = None;
    let (_, settled) = arms.draw(&mut t);
    assert!(
        differing(&mid_in_place, &settled) > 0,
        "the slide did not move the screen"
    );

    // Rank 1: the overlay records slide with the grid. The plain-digit gates
    // above prove the cell math; this proves a combining mark and a
    // cross-cell ligature on the *visible* rows move with the glyphs beneath
    // them, rather than holding their un-slid positions for the ~60 ms of a
    // slide. The marked and ligatured cells are addressed absolutely into the
    // middle of the live screen, so the leaving row (one above the window)
    // and the bottom row are plain by construction: rank 1 slides only the
    // on-screen records.
    //
    // Do not "fix" the plain leaving row by marking it -- a mark there goes
    // red and *stays* red, even with a leaving-row overlay pass built. The
    // reason is the shader, not a missing pass: `cell_glyph_vertex` consults
    // the overlay mask only for `lrow >= 0` (the mask is `cols*rows` and has
    // no slot for the leaving row at `lrow = -1`), so it always draws the
    // leaving row's base glyph. An overlay record for that row would draw the
    // base a second time, and this full-slide-vs-one-line-up gate then
    // differs at the base's antialiased edges where the reference masks it.
    // Masking the leaving row needs a `cols*(rows+1)` mask with every index
    // shifted *and* a shader change -- see CLAUDE.md "Rank 2 and 3 are closed
    // on the shader". That is why rank 1 stops at the on-screen rows.
    feed(&mut t, b"\x1b[?25l\x1b[2J\x1b[H");
    for i in 0..14u32 {
        feed(
            &mut t,
            format!("plain filler row {i:02} here\r\n").as_bytes(),
        );
    }
    // Middle of the 10-row live screen: `e` + U+0301 on row 5 and a `->`
    // ligature on row 6, the shapes the resting marked and ligature rows use.
    feed(&mut t, b"\x1b[5;1Hmark e\xcc\x81 here");
    feed(&mut t, b"\x1b[6;1Harrow a -> b here");
    arms.map.ensure(
        t.view(Rows::of_count(1)),
        &mut arms.renderer,
        &mut arms.candidates,
    );
    // A full-row slide at the live screen is the screen one line up -- the
    // strong reference, drawn with no slide shader at all. If the records do
    // not slide, the mark's accent and the ligature's glyphs stay at their
    // pre-slide rows while the grid moves under them, and the two frames
    // differ at exactly those cells.
    arms.back = Rows::ZERO;
    arms.slide = Some(cell_h);
    let (_, slid_marks) = arms.draw(&mut t);
    assert!(
        arms.exceptions.stats().records.value() > 0,
        "the marked and ligatured slide scene emitted overlay records"
    );
    arms.slide = None;
    arms.back = Rows::of_count(1);
    let (_, up_marks) = arms.draw(&mut t);
    let (w, _) = arms.renderer.pixel_size();
    let m = arms.renderer.metrics();
    let cell = (
        usize::try_from(m.cell_w.value()).unwrap(),
        usize::try_from(m.cell_h.value()).unwrap(),
    );
    let bx = diff_box(
        &slid_marks,
        &up_marks,
        usize::try_from(w.value()).unwrap(),
        cell,
    );
    assert_eq!(
        differing(&slid_marks, &up_marks),
        0,
        "the overlay records did not slide with the grid, cells {bx:?}"
    );
    // At rest the pan is zero and the records sit exactly where the instance
    // path puts them: the same marked and ligatured content, no slide, must
    // agree across the two arms -- the cell-vs-instance pairing the slide gate
    // above deliberately does not use, closing the zero-pan direction of the
    // field.
    arms.back = Rows::ZERO;
    arms.slide = None;
    let (rest_a, rest_b) = arms.draw(&mut t);
    assert_eq!(
        differing(&rest_a, &rest_b),
        0,
        "the marked and ligatured content disagrees across the arms at rest"
    );

    // **The picture gate.** Under `app_backgrounds = "dim"` a picture drawn
    // in text -- a connected block of set backgrounds holding sixteen
    // colours or more -- keeps its backgrounds solid, and nothing else does.
    // The instance arm forces that in `convert_row` and the cell arm in
    // `cell_bg_vertex` from the overlay's `MASK_PICTURE` bit: two spellings
    // of one rule, which is what this compares. The scene holds the three
    // shapes the rule decides between -- a 2x20 block of forty colours, a
    // row of fifteen (one short of a picture), and a one-colour panel -- and
    // is drawn under both `pictures` settings, so a cell arm that ignored
    // the bit would agree with the instance arm under `dim` and not under
    // `opaque`.
    let resting = arms.renderer.theme().clone();
    feed(&mut t, b"\x1b[?25l\x1b[2J\x1b[H");
    for y in 0..2u32 {
        feed(&mut t, format!("\x1b[{};3H", y + 2).as_bytes());
        for x in 0..20u32 {
            let (r, g, b) = (x * 12, y * 120 + 40, 255 - x * 12);
            feed(
                &mut t,
                format!("\x1b[38;2;{b};{r};{g};48;2;{r};{g};{b}m\u{2580}").as_bytes(),
            );
        }
        feed(&mut t, b"\x1b[0m");
    }
    feed(&mut t, b"\x1b[5;3H");
    for x in 0..15u32 {
        feed(
            &mut t,
            format!("\x1b[48;2;{};90;90m \x1b[0m", x * 16 + 10).as_bytes(),
        );
    }
    feed(&mut t, b"\x1b[7;3H\x1b[44m  panel  \x1b[0m");
    let (cols, _) = arms.renderer.grid();
    let mut frames = Vec::new();
    for (pictures, want_pictured) in [
        (baud_render::PictureBg::Opaque, 40),
        (baud_render::PictureBg::Dimmed, 0),
    ] {
        arms.renderer.set_theme(Theme {
            app_bg: baud_render::AppBg::Dimmed,
            picture_bg: pictures,
            ..resting.clone()
        });
        let (a, b) = arms.draw(&mut t);
        assert_eq!(
            arms.exceptions.stats().pictured.value(),
            want_pictured,
            "{pictures:?}: the cell arm's overlay found the picture's cells"
        );
        let bx = diff_box(
            &a,
            &b,
            usize::try_from(arms.renderer.pixel_size().0.value()).unwrap(),
            (
                usize::try_from(arms.renderer.metrics().cell_w.value()).unwrap(),
                usize::try_from(arms.renderer.metrics().cell_h.value()).unwrap(),
            ),
        );
        assert_eq!(
            differing(&a, &b),
            0,
            "{pictures:?}: the arms disagree over a text-drawn picture, cells {bx:?}"
        );
        // The instance arm's own backgrounds, cell by cell: the picture
        // solid only under `opaque`, the fifteen-colour row and the panel
        // dimmed under both.
        let bg = arms.renderer.background();
        let alpha = |row: usize, col: usize| bg[row * cols.index() + col].0.to_be_bytes()[0];
        let solid = if want_pictured > 0 { 0xFF } else { 56 };
        assert_eq!(alpha(1, 2), solid, "{pictures:?}: the picture's first cell");
        assert_eq!(alpha(2, 21), solid, "{pictures:?}: the picture's last cell");
        assert_eq!(
            alpha(4, 2),
            56,
            "{pictures:?}: fifteen colours are not a picture"
        );
        assert_eq!(alpha(6, 3), 56, "{pictures:?}: a panel is not a picture");
        assert_eq!(
            alpha(1, 1),
            56,
            "{pictures:?}: the default background beside it"
        );
        frames.push(b);
    }
    assert!(
        differing(&frames[0], &frames[1]) > 0,
        "the picture's solid backgrounds did not reach the cell arm's pixels"
    );
    arms.renderer.set_theme(resting);

    // **The image gate, last so a lingering placement disturbs nothing.** A
    // real Sixel homed onto the live screen: both GPU arms draw it through the
    // same `Kind::Color` pass, so they must agree pixel for pixel -- and it
    // must *change its rect* against the very same frame with no image, or a
    // pass that drew nothing (both arms agreeing on no image, the `golden.rs`
    // vacuity) would pass while proving the opposite. The rect already carries
    // text under it, so a bare colour count cannot tell an image from the text
    // it covers -- only a before/after can.
    feed(&mut t, b"\x1b[H\x1b[?25l");
    let (_, no_image) = arms.draw(&mut t);
    feed(&mut t, SWIRL);
    let (img_a, img_b) = arms.draw(&mut t);
    let places = arms.renderer.image_placements();
    assert_eq!(places.len(), 1, "the sixel was placed on the live screen");
    let p = places[0];
    assert_eq!(
        differing(&img_a, &img_b),
        0,
        "the arms disagree with the sixel on screen"
    );
    let iw = usize::try_from(arms.renderer.pixel_size().0.value()).unwrap();
    let (rx, ry) = (
        usize::try_from(p.x).unwrap_or(0),
        usize::try_from(p.y).unwrap_or(0),
    );
    let (rw, rh) = (usize::from(p.w), usize::from(p.h));
    let mut changed = 0;
    for y in ry..ry + rh {
        for x in rx..rx + rw {
            let i = y * iw + x;
            if no_image.get(i) != img_b.get(i) {
                changed += 1;
            }
        }
    }
    assert!(
        changed > rw * rh / 2,
        "the sixel changed its rect against the no-image frame: {changed} of {} pixels",
        rw * rh
    );

    // **An image slides with its line.** The same strong reference the
    // grid's slide gate uses: a full-row slide at the live screen is the
    // screen one line up, drawn with no slide shader at all. `RIS` drops the
    // swirl, then plain digit lines build history, and a one-band 8x6 Sixel
    // -- shorter than a cell, so it lies inside one row -- is placed on the
    // bottom row. Newlines alone scroll it up: an `LF` writes no cell, so
    // nothing is drawn over the image and its row is never blanked.
    feed(&mut t, b"\x1bc\x1b[?25l");
    for i in 0..14u32 {
        feed(
            &mut t,
            format!("0000 1111 2222 row {i:02} 3333 4444 5555\r\n").as_bytes(),
        );
    }
    feed(&mut t, DOT);
    let placed = |t: &Term<'_>| {
        t.images()
            .placements()
            .map(|(_, p)| p.line)
            .next()
            .expect("the dot was placed")
    };
    let line = placed(&t);
    let cell_h = arms.renderer.metrics().cell_h;
    let slid_is_one_up = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, what: &str| {
        arms.map.ensure(
            t.view(Rows::of_count(1)),
            &mut arms.renderer,
            &mut arms.candidates,
        );
        arms.back = Rows::ZERO;
        arms.slide = Some(cell_h);
        let (_, slid) = arms.draw(t);
        arms.slide = None;
        arms.back = Rows::of_count(1);
        let (_, one_up) = arms.draw(t);
        // The image is on the reference frame, so an equal pair is two
        // drawings of it and not two frames without it.
        assert_eq!(
            arms.renderer.image_placements().len(),
            1,
            "{what}: the dot is drawn on the frame one line up"
        );
        arms.back = Rows::ZERO;
        let d = differing(&slid, &one_up);
        assert_eq!(
            d, 0,
            "{what}: a full-row slide is not the screen one line up, {d} pixels"
        );
    };
    // Mid-screen: scroll the image up from the bottom row to row 4.
    while t.image_origin(Rows::ZERO).top.value() + 4 < line.value() {
        feed(&mut t, b"\n");
    }
    slid_is_one_up(&mut arms, &mut t, "an image mid-screen");
    // The leaving row: the image's line has just scrolled off the top, so at
    // rest it is not on screen, and during the slide it is -- in the one row
    // the slide draws above the window. A cull that ignores the slide drops
    // it, and it pops off at the start of the slide instead of sliding out.
    while t.image_origin(Rows::ZERO).top.value() <= line.value() {
        feed(&mut t, b"\n");
    }
    assert_eq!(
        t.image_origin(Rows::ZERO).top.value(),
        line.value() + 1,
        "the dot's line is the leaving row"
    );
    assert_eq!(placed(&t), line, "the dot survived the scroll");
    slid_is_one_up(&mut arms, &mut t, "an image in the leaving row");
}

/// A one-band Sixel, 8 pixels wide and 6 high: shorter than any cell this
/// file draws, so the whole image lies inside one row.
const DOT: &[u8] = b"\x1bPq\"1;1;8;6#1;2;100;0;0#1!8~\x1b\\";
