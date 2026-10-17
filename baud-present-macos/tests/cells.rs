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
use baud_domain::{
    BytePos, Chan, Col, Cols, Depth, ImageAlpha, ImageBudget, ImagePx, PackedCell, Pixel, ReadLen,
    Row, RowRot, Rows, WinSize,
};
use baud_font::{Face, Px};
use baud_present_macos::{
    CellInput, Done, Drawn, Gpu, GridBuffer, MapBuffer, OverlayBuffer, StylesBuffer, premultiply,
};
use baud_render::effect::{Effect, Ignition, Seed, Vfx};
use baud_render::fire;
use baud_render::ice;
use baud_render::lightning;
use baud_render::liquid;
use baud_render::orb;
use baud_render::plasma;
use baud_render::ufo;
use baud_render::{
    Argb, Blend, Blink, BlinkPhases, Candidate, Composition, Corners, CursorPhase, Damage,
    FaceRole, FaceStyle, GlyphMap, Highlight, Hover, Instance, Kind, Look, Overlay, Pt, PxSize,
    Rect, Region, Renderer, Resolved, Scene, Scroll, Scroller, Segment, Shape, Smear, Stack,
    StyleStamp, Target, Theme, Trail, TrailLook, Transition, VideoFlash, Widths, Window,
    copy_for_shader, quad_coverage,
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
/// `blue bg` and `reverse` spans. That row ends on palette entries 0, 7, 8
/// and 15, and the scene holds SGR, `38;5`, `48;5` and truecolour cells, so
/// the reverse-video scene draws every cell's exchange (DEC STD 070
/// § 5.6.3.2; the owner, 2026-10-13) through both arms and not only the
/// defaults. Both read it from one resolved table, so this compares their
/// drawing of it; the exchange's own rule is gated in
/// `baud-render/tests/reverse_video.rs`.
fn scene(t: &mut Term<'_>) {
    feed(
        t,
        b"Hello, world. 0123456789 abc XYZ (x) [y] {z}\r\n\
          \x1b[31mred \x1b[1;32mbold green\x1b[0m \x1b[4munderline\x1b[0m \x1b[9mstrike\x1b[0m\r\n\
          \x1b[4:3mcurly\x1b[0m \x1b[4:4mdotted\x1b[0m \x1b[4:5mdashed\x1b[0m \x1b[58;5;196;4:3mred curl\x1b[0m\r\n\
          \x1b[44m blue bg \x1b[7m reverse \x1b[0m \x1b[2mdim\x1b[0m \x1b[8mhidden\x1b[0m done \x1b[30;47m ink \x1b[90;107m pale \x1b[0m\r\n\
          \x1b[38;2;200;100;50mtrue colour\x1b[0m \x1b[48;5;27;38;5;226mindexed\x1b[0m \x1b[58;5;196;4mul red\x1b[0m\r\n\
          box \xe2\x94\x8c\xe2\x94\x80\xe2\x94\x90 braille \xe2\xa3\xbf end \x1b[38;2;200;0;0;48;2;0;0;200m\xe2\x96\x80\xe2\x96\x80 \xe2\x94\x80\x1b[0;7;32m\xe2\x96\x84\x1b[0m image\r\n\
          \xe2\x94\x8f\xe2\x94\x81\xe2\x94\xb3\xe2\x94\x93 \xe2\x95\x94\xe2\x95\x90\xe2\x95\xa6\xe2\x95\x97 \xe2\x95\xad\xe2\x94\x80\xe2\x95\xae \xe2\x95\xb1\xe2\x95\xb2\xe2\x95\xb3 \xe2\x94\x84\xe2\x94\x8c\xe2\x95\x8c \xe2\x96\x80\xe2\x96\x84\xe2\x96\x88\xe2\x96\x8c\xe2\x96\x90\xe2\x96\x91\xe2\x96\x92\xe2\x96\x93\xe2\x96\x9a\xe2\x96\x9f \xe2\xa0\x81\xe2\xa0\xbf\xe2\xa3\x80\xe2\xa2\xb2 \x1b[31m\xe2\x94\x8f\xe2\x94\x81\xe2\x94\x93\x1b[0m\r\n\
          marks e\xcc\x81 a\xcc\x88 o\xcc\x82 q\xcc\x96\xcc\x81 composed\r\n\
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
    /// Where the window sits and how the thumb is seen, for the scrollbar
    /// both arms draw.
    window: Option<Scroll>,
    /// The cursor trail both arms draw, or none.
    trail: Option<TrailLook>,
    /// The alternate-screen fade both arms draw, or none.
    fade: Option<Transition>,
    /// An input method's composition both arms draw, or none.
    composition: Option<Composition>,
    /// The window copied flat, in screen order -- research decision D4's
    /// buffer, and what the app binds. Held here so a third arm can draw
    /// from it and be compared against the two that read the live grid.
    copy: GridBuffer,
    copy_mem: &'static mut [PackedCell],
    /// Smooth scroll: the pixels the screen is shifted down, or none. Both
    /// cell arms draw one extra leaving row above the window when set.
    slide: Option<Px>,
    /// Where the cell arm's resolved table stands: kept across scenes, as the
    /// app keeps a set's, so each scene's frame resolves only what it wrote
    /// or what an input moved -- the cache the app draws from.
    stamp: StyleStamp,
    /// Pixels the target is wider and taller than the frame, which both arms
    /// letterbox to its centre: a scene that sets it draws at a frame origin
    /// other than zero, which a fragment function that forgot the origin
    /// would misplace in both arms alike.
    margin: (u32, u32),
}

impl Arms<'_, '_> {
    /// Both arms drawn offscreen and read back.
    fn draw(&mut self, t: &mut Term<'_>) -> (Vec<u32>, Vec<u32>) {
        let (fw, fh) = self.renderer.frame_size();
        let (w, h) = (
            Px::of(fw.value() + self.margin.0),
            Px::of(fh.value() + self.margin.1),
        );
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
        self.renderer.set_composition(self.composition.clone());
        // Both arms draw the same image placements: computed once here, read by
        // `draw` and `draw_cells` alike.
        self.renderer.set_images(t, self.back);
        // The styles before the map walk, as the app orders them: the walk
        // reads each cell's face style from the synced table.
        self.renderer
            .sync_styles(t, self.styles_mem, &mut self.stamp);
        // The app's rule: the window by the core's clamp, so the alternate
        // screen -- which keeps no history -- has its cursor whatever
        // window the scene carries.
        let live = t.looking_back(self.back).index() == 0;
        let composition = self.composition.clone().filter(|_| live);
        let shown = (t.cursor_visible() && live).then(|| {
            let (r, c) = match &composition {
                Some(c) => c.cursor(view.cols()),
                None => t.cursor(),
            };
            (r, c, t.cursor_style())
        });
        self.map.ensure(
            t,
            view,
            self.styles_mem,
            &mut self.renderer,
            &mut self.candidates,
        );
        let cursor = shown.filter(|&(_, _, style)| self.renderer.cursor_drawn(style));
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
                composition: composition.as_ref(),
            },
            &mut self.renderer,
            self.overlay_mem,
            &mut self.records,
        );
        // The composition as the app draws it on the cell path: its cells
        // masked, its records after the walk's.
        if let Some(c) = &composition {
            let under = view
                .row(c.row)
                .get(c.col.index())
                .copied()
                .unwrap_or(PackedCell::EMPTY);
            self.exceptions.composed(
                &mut self.renderer,
                c,
                under,
                self.styles_mem,
                cursor,
                view.cols(),
                self.slide.unwrap_or(Px::of(0)),
                self.overlay_mem,
                &mut self.records,
            );
        }
        self.records.extend_from_slice(self.renderer.trail());
        // The plasma blink's glow, from the cursor before the phase's
        // filter, as the app builds it.
        self.renderer.set_cursor_glow_at(shown);
        self.records.extend_from_slice(self.renderer.cursor_glow());
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
            cursor_color: t.cursor_color_in(self.renderer.video()),
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
        let (w, h) = self.renderer.frame_size();
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
        // with the identity rotation. Copied as the app copies it, a marked
        // cell given its base (`copy_for_shader`).
        let ring = if self.slide.is_some() {
            let above = t.view(Rows::of_index(self.back.index() + 1));
            let src = above.row(Row::from_index(0));
            let at = rows * stride;
            copy_for_shader(t, &src[..cols], &mut self.copy_mem[at..at + cols]);
            rows + 1
        } else {
            rows
        };
        // The app's rule: the window by the core's clamp, so the alternate
        // screen -- which keeps no history -- has its cursor whatever
        // window the scene carries.
        let live = t.looking_back(self.back).index() == 0;
        let cursor = (t.cursor_visible() && live)
            .then(|| {
                let (r, c) = match self.composition.as_ref().filter(|_| live) {
                    Some(c) => c.cursor(view.cols()),
                    None => t.cursor(),
                };
                (r, c, t.cursor_style())
            })
            .filter(|&(_, _, style)| self.renderer.cursor_drawn(style));
        let input = CellInput {
            set: baud_present_macos::journal::SetIx(0),
            grid: &self.copy,
            styles: &self.styles,
            map: &self.map_buffer,
            stride: t.stride(),
            rotation: RowRot::NONE,
            ring: Rows::of_index(ring),
            cursor,
            cursor_color: t.cursor_color_in(self.renderer.video()),
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

/// A hash word, for the effects' references below: the same PCG output
/// permutation `fx_hash` is, spelled a third time on purpose -- a reference
/// that called the renderer's would agree with it by construction.
#[derive(Clone, Copy)]
struct Word(u32);

impl Word {
    fn hash(self) -> Word {
        let s = self.0.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
        let w = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
        Word((w >> 22) ^ w)
    }

    fn unit(self) -> f32 {
        f32::from_bits((self.0 >> 9) | 0x3f80_0000) - 1.0
    }

    fn plus(self, o: Word) -> Word {
        Word(self.0.wrapping_add(o.0))
    }
}

/// `fx_lattice` at the lattice point `(x, y)`, whole numbers as floats.
fn fx_lattice(x: f32, y: f32, seed: Word) -> f32 {
    let (ix, iy) = (
        Word((x as i32).cast_unsigned()),
        Word((y as i32).cast_unsigned()),
    );
    ix.plus(iy.plus(seed).hash()).hash().unit()
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// `fx_noise`: value noise with the quintic fade.
fn fx_noise(p: Pt, seed: Word) -> f32 {
    let (fx, fy) = (p.x.floor(), p.y.floor());
    let (x, y) = (p.x - fx, p.y - fy);
    let fade = |v: f32| v * v * v * (v * (v * 6.0 - 15.0) + 10.0);
    let (wx, wy) = (fade(x), fade(y));
    let a = fx_lattice(fx, fy, seed);
    let b = fx_lattice(fx + 1.0, fy, seed);
    let c = fx_lattice(fx, fy + 1.0, seed);
    let d = fx_lattice(fx + 1.0, fy + 1.0, seed);
    mix(mix(a, b, wx), mix(c, d, wx), wy)
}

/// `fx_fbm`: four rotated octaves.
fn fx_fbm(mut p: Pt, seed: Word) -> f32 {
    let (mut t, mut a) = (0.0f32, 0.5f32);
    for o in 0..4u32 {
        t += a * fx_noise(p, seed.plus(Word(o.wrapping_mul(0x9E37_79B9))));
        p = Pt {
            x: (0.8 * p.x - 0.6 * p.y) * 2.02,
            y: (0.6 * p.x + 0.8 * p.y) * 2.02,
        };
        a *= 0.5;
    }
    t
}

/// `fx_distance`: the signed distance to the quad by the non-zero rule.
fn fx_distance(p: Pt, c: &Corners) -> f32 {
    let q = c.0;
    let (mut d, mut winding) = (f32::INFINITY, 0i32);
    for i in 0..4 {
        let (a, b) = (q[i], q[(i + 1) % 4]);
        let (bx, by) = (b.x - a.x, b.y - a.y);
        let (px, py) = (p.x - a.x, p.y - a.y);
        let h = ((px * bx + py * by) / (bx * bx + by * by).max(1e-6)).clamp(0.0, 1.0);
        let (dx, dy) = (px - bx * h, py - by * h);
        d = d.min((dx * dx + dy * dy).sqrt());
        let side = bx * py - by * px;
        if a.y <= p.y {
            if b.y > p.y && side > 0.0 {
                winding += 1;
            }
        } else if b.y <= p.y && side < 0.0 {
            winding -= 1;
        }
    }
    if winding != 0 { -d } else { d }
}

/// **The fire's reference**: `fire_flame` for the pixel centred at `p` in
/// frame pixels, premultiplied, in `color`'s channels 0 to 1.
fn fire_reference(vfx: &Vfx, p: Pt, color: [f32; 4]) -> [f32; 4] {
    let (e, f) = (vfx.emitter, vfx.ignition);
    if e.hole.contains(p) {
        return [0.0; 4];
    }
    let k = f.age / fire::LIFE;
    if k >= 1.0 {
        return [0.0; 4];
    }
    let c = f.unit;
    let d0 = fx_distance(p, &e.corners) / c;
    if d0 > fire::REACH {
        return [0.0; 4];
    }
    // The noise is anchored on the ride, `k[3]`, as the shader reads it.
    let rode = vfx.params()[3];
    let s = Pt {
        x: p.x / c * 2.6 - f.dir.x * (f.age * fire::DRIFT),
        y: ((p.y - rode) / c + f.age * fire::RISE) * 0.7 - f.dir.y * (f.age * fire::DRIFT),
    };
    let n = fx_fbm(s, Word(f.seed.0));
    let lift = fire::HEIGHT * c * smoothstep(0.25, 0.85, n);
    let d = fx_distance(
        Pt {
            x: p.x,
            y: p.y + lift,
        },
        &e.corners,
    ) / c;
    let body = (1.0 - smoothstep(-0.10, 0.20, d)).max(0.8 * (1.0 - smoothstep(-0.25, 0.05, d0)));
    let cool = (d0.max(0.0) / fire::HEIGHT).clamp(0.0, 1.0);
    let u = (((p.x - f.tail.x) * f.dir.x + (p.y - f.tail.y) * f.dir.y) / f.len.max(1.0))
        .clamp(0.0, 1.0);
    let burn = smoothstep(k - 0.15, k + 0.35, u) * (1.0 - k * k);
    let heat = (body * (1.0 - 0.85 * cool) * (0.50 + 0.60 * n) * burn).clamp(0.0, 1.0);
    let halo = 0.12 * (-2.5 * d0.max(0.0)).exp() * burn;
    let a = color[3] * (smoothstep(0.05, 0.95, heat) + halo).clamp(0.0, 1.0);
    [color[0] * a, color[1] * a, color[2] * a, a]
}

/// `px_wave`: sin(2 pi x) with the argument folded into [-1, 1) first, by
/// MSL's `fract` -- `x - floor(x)`, below one -- which `f32::fract` is not
/// for a negative `x`.
fn px_wave(x: f32) -> f32 {
    let f = (x + 0.5 - (x + 0.5).floor()).min(1.0 - f32::EPSILON / 2.0);
    (core::f32::consts::PI * (2.0 * f - 1.0)).sin()
}

/// The filaments' table, spelled as the shader spells it: cycles a cell and
/// a second of each wave and harmonic, the harmonic's share, the width in
/// cells and the gain.
const PX_K: [f32; 3] = [0.23, 0.31, 0.17];
const PX_F: [f32; 3] = [1.10, 1.35, 0.80];
const PX_K2: [f32; 3] = [0.71, 0.93, 0.53];
const PX_F2: [f32; 3] = [-0.85, -1.05, -1.20];
const PX_M: [f32; 3] = [0.22, 0.20, 0.25];
const PX_W: [f32; 3] = [0.040, 0.034, 0.050];
const PX_G: [f32; 3] = [1.00, 0.85, 0.65];

/// The plasma's parts at `p`: the envelope, the tube with its sheath, the
/// filaments, and how far behind the head the pixel is along the axis in
/// cells -- `None` where the fragment function draws nothing.
fn plasma_parts(vfx: &Vfx, p: Pt) -> Option<(f32, f32, f32, f32)> {
    let (e, f) = (vfx.emitter, vfx.ignition);
    if e.hole.contains(p) {
        return None;
    }
    let k = f.age / plasma::LIFE;
    if k >= 1.0 {
        return None;
    }
    let c = f.unit;
    let d0 = fx_distance(p, &e.corners) / c;
    if d0 > plasma::REACH {
        return None;
    }
    let q = vfx.params();
    let env = (1.0 - k) * (1.0 - k);
    let inside = 1.0 - smoothstep(-0.03, 0.03, d0);
    let tube = q[3] * inside + q[4] * (-q[5] * (1.0 + 1.5 * k) * d0.max(0.0)).exp();
    let (ax, ay) = (q[1], q[2]);
    let (rx, ry) = (
        p.x - 0.5 * (e.hole.left + e.hole.right),
        p.y - 0.5 * (e.hole.top + e.hole.bottom),
    );
    let s = -(rx * ax + ry * ay) / c;
    let mut fil = 0.0f32;
    if d0 < 0.15 {
        let w = (ax * ry - ay * rx) / c;
        let amp = q[0] * 0.62 * smoothstep(0.0, 2.0, s);
        let salt = Word(f.seed.0 ^ 0xA511_E9B3);
        let kn = salt.plus(Word(8)).hash().unit();
        for i in 0..3usize {
            let n = u32::try_from(i).unwrap();
            let ph = salt.plus(Word(n)).hash().unit();
            let ph2 = salt.plus(Word(16 + n)).hash().unit();
            let along = amp
                * ((1.0 - PX_M[i]) * px_wave(PX_K[i] * s + ph + PX_F[i] * f.age)
                    + PX_M[i] * px_wave(PX_K2[i] * s + ph2 + PX_F2[i] * f.age));
            let wid = (PX_W[i] * (1.0 - 0.5 * k)).max(1.1 / c);
            let x = (w - along) / wid;
            let knot = 0.72 + 0.28 * px_wave(0.45 * s + kn + n as f32 / 3.0 + q[7] * f.age);
            fil += PX_G[i] * (-x * x).exp() * knot;
        }
        fil = fil.clamp(0.0, 1.0)
            * q[6]
            * (1.0 - smoothstep(-0.02, 0.15, d0))
            * smoothstep(0.0, 0.3, s);
    }
    Some((env, tube, fil, s))
}

/// The plasma's tube and sheath without its filaments: what must be all
/// there is ahead of the cursor.
fn plasma_sheath(vfx: &Vfx, p: Pt, color: [f32; 4]) -> [f32; 4] {
    let Some((env, tube, _, _)) = plasma_parts(vfx, p) else {
        return [0.0; 4];
    };
    let a = color[3] * (env * tube).clamp(0.0, 1.0);
    [color[0] * a, color[1] * a, color[2] * a, a]
}

/// **The plasma's reference**: `plasma_tube` for the pixel centred at `p`.
fn plasma_reference(vfx: &Vfx, p: Pt, color: [f32; 4]) -> [f32; 4] {
    let Some((env, tube, fil, _)) = plasma_parts(vfx, p) else {
        return [0.0; 4];
    };
    let a = color[3] * (env * (tube + (1.0 - tube) * fil)).clamp(0.0, 1.0);
    [color[0] * a, color[1] * a, color[2] * a, a]
}

/// **The lightning's reference**: `bolt_light`, through the renderer's own
/// Rust model of it, `lightning::light` -- which the unit tests hold to the
/// bolt's properties, so the oracle ties the shader to them.
fn lightning_reference(vfx: &Vfx, p: Pt, color: [f32; 4]) -> [f32; 4] {
    let a = color[3] * lightning::light(vfx, p);
    [color[0] * a, color[1] * a, color[2] * a, a]
}

/// **The ice's reference**: `ice_frost`, through the renderer's own Rust
/// model of it, `ice::light`.
fn ice_reference(vfx: &Vfx, p: Pt, color: [f32; 4]) -> [f32; 4] {
    let a = color[3] * ice::light(vfx, p);
    [color[0] * a, color[1] * a, color[2] * a, a]
}

/// An ice shard: the hexagonal plate `ice_fragment` draws a particle as,
/// through `ice::shard`.
fn ice_shard(uv: Pt, color: [f32; 4]) -> [f32; 4] {
    let a = color[3] * ice::shard(uv);
    [color[0] * a, color[1] * a, color[2] * a, a]
}

/// **The ufo's reference**: `ufo_fragment`'s body, through `ufo::light`.
fn ufo_reference(vfx: &Vfx, p: Pt, color: [f32; 4]) -> [f32; 4] {
    let a = color[3] * ufo::light(vfx, p);
    [color[0] * a, color[1] * a, color[2] * a, a]
}

/// An exhaust ring, through `ufo::ring`.
fn ufo_ring(uv: Pt, color: [f32; 4]) -> [f32; 4] {
    let a = color[3] * ufo::ring(uv);
    [color[0] * a, color[1] * a, color[2] * a, a]
}

/// **The orb's reference**: `orb_light`, through `orb::light`.
fn orb_reference(vfx: &Vfx, p: Pt, color: [f32; 4]) -> [f32; 4] {
    let a = color[3] * orb::light(vfx, p);
    [color[0] * a, color[1] * a, color[2] * a, a]
}

/// **The liquid's reference**: `liquid_light`, through `liquid::light`.
fn liquid_reference(vfx: &Vfx, p: Pt, color: [f32; 4]) -> [f32; 4] {
    let a = color[3] * liquid::light(vfx, p);
    [color[0] * a, color[1] * a, color[2] * a, a]
}

/// A record's colour as the shader reads it: channels 0 to 1, alpha last.
fn unit_color(c: Argb) -> [f32; 4] {
    let f = |v: Chan| f32::from(v.get()) / 255.0;
    [f(c.r()), f(c.g()), f(c.b()), f(c.a())]
}

/// Premultiplied `src` over the stored pixel `dst`, rounded to the target's
/// bytes the way a blend unit stores them.
fn over(src: [f32; 4], dst: u32) -> u32 {
    let ch = |k: u32| f32::from(u8::try_from((dst >> k) & 0xff).unwrap_or(0)) / 255.0;
    let (r, g, b, a) = (ch(16), ch(8), ch(0), ch(24));
    let mix = |s: f32, d: f32| {
        let v = ((s + d * (1.0 - src[3])) * 255.0).round().clamp(0.0, 255.0);
        u32::try_from(v as i64).unwrap_or(0)
    };
    (mix(src[3], a) << 24) | (mix(src[0], r) << 16) | (mix(src[1], g) << 8) | mix(src[2], b)
}

/// `fx_particle`: a soft disc over its record, at `uv` -1 to 1 on each axis.
fn fx_particle(uv: Pt, color: [f32; 4]) -> [f32; 4] {
    let a = 1.0 - smoothstep(0.15, 1.0, (uv.x * uv.x + uv.y * uv.y).sqrt());
    let a = color[3] * a * a;
    [color[0] * a, color[1] * a, color[2] * a, a]
}

/// **An effect by its reference**: the band's rule for the pixel centred at
/// `p` in frame pixels, and the particle's for a pixel at `uv` in its
/// record, each premultiplied, in `color`'s channels 0 to 1.
struct Reference {
    band: fn(&Vfx, Pt, [f32; 4]) -> [f32; 4],
    particle: fn(Pt, [f32; 4]) -> [f32; 4],
}

/// **Every effect rides rigidly**: a look translated by `d` is the look
/// before, drawn `d` lower -- `band(v.translated(d), p + (0, d)) == band(v,
/// p)` at every sample, and every particle moved by exactly `(0, d)`. That is
/// what lets a trail ride a smooth scroll with the text. The fire failed it
/// until its noise was anchored on the ride (`Ignition::rode`, `k[3]`): its
/// flame was sampled from noise fixed to the frame, so a translated fire
/// crawled through it. The ufo is placed away from the top, where its hover
/// side reads the hole's absolute row (`ufo.rs`), the recorded exception.
#[test]
fn every_effect_rides_rigidly() {
    let (cw, ch) = (17.0f32, 34.0f32);
    let cell = |col: f32, row: f32| Rect {
        left: col * cw,
        top: row * ch,
        right: (col + 1.0) * cw,
        bottom: (row + 1.0) * ch,
    };
    let centre = |r: Rect| Pt {
        x: (r.left + r.right) * 0.5,
        y: (r.top + r.bottom) * 0.5,
    };
    let (from, to) = (cell(10.0, 9.0), cell(30.0, 6.0));
    let seg = Segment::new(from.corners(), to);
    let colour = [0.9f32, 0.8, 0.7, 1.0];
    for effect in Effect::ALL {
        let reference = reference(effect);
        let end = effect.end(&Ignition::ignite(centre(from), to, Seed(7), ch));
        for age in [0.15f32, 0.3].map(|f| f * end) {
            let vfx = Vfx {
                effect,
                emitter: Smear {
                    corners: seg.at(age),
                    hole: to,
                    shape: Shape::Quad,
                },
                ignition: Ignition::ignite(centre(from), to, Seed(7), ch).at(age),
                axis: Pt { x: 0.8, y: -0.6 },
            };
            for d in [-2.0 * ch, -ch, 13.0] {
                let moved = vfx.translated(d);
                let mut worst = 0.0f32;
                let mut lit = 0usize;
                for y in (4..14).map(|r| r as f32 * ch) {
                    for yy in (0..34).step_by(3) {
                        for x in (8..34).flat_map(|c| {
                            (0..17).step_by(4).map(move |xx| c as f32 * cw + xx as f32)
                        }) {
                            let p = Pt {
                                x: x + 0.5,
                                y: y + yy as f32 + 0.5,
                            };
                            let a = (reference.band)(&vfx, p, colour);
                            let b = (reference.band)(&moved, Pt { x: p.x, y: p.y + d }, colour);
                            lit += usize::from(a[3] > 0.01);
                            for i in 0..4 {
                                worst = worst.max((a[i] - b[i]).abs());
                            }
                        }
                    }
                }
                assert!(
                    worst <= 1.0 / 1024.0,
                    "{effect:?} at {age} moved {d}: a channel moved by {worst}"
                );
                let (mut before, mut after) = (Vec::new(), Vec::new());
                vfx.particles(|q| before.push(q));
                moved.particles(|q| after.push(q));
                assert_eq!(
                    before.len(),
                    after.len(),
                    "{effect:?} at {age}: the particles"
                );
                for (q, r) in before.iter().zip(&after) {
                    assert!(
                        (r.centre.x - q.centre.x).abs() < 1e-3
                            && (r.centre.y - q.centre.y - d).abs() < 1e-3
                            && r.radius == q.radius
                            && r.glow == q.glow,
                        "{effect:?} at {age}: a particle did not move by {d}: {q:?} against {r:?}"
                    );
                }
                // A control: the samples cover something drawn, or the
                // comparison is between two blanks.
                assert!(lit >= 8, "{effect:?} at {age}: only {lit} samples lit");
            }
        }
    }
}

/// The reference for an effect: every effect has one, so a new effect
/// without one does not compile.
fn reference(effect: Effect) -> Reference {
    match effect {
        Effect::Fire => Reference {
            band: fire_reference,
            particle: fx_particle,
        },
        Effect::Plasma => Reference {
            band: plasma_reference,
            particle: fx_particle,
        },
        Effect::Lightning => Reference {
            band: lightning_reference,
            particle: fx_particle,
        },
        Effect::Ice => Reference {
            band: ice_reference,
            particle: ice_shard,
        },
        Effect::Ufo => Reference {
            band: ufo_reference,
            particle: ufo_ring,
        },
        Effect::Orb => Reference {
            band: orb_reference,
            particle: fx_particle,
        },
        Effect::Liquid => Reference {
            band: liquid_reference,
            particle: fx_particle,
        },
    }
}

/// **An effect's frame by its reference**: the records the renderer built,
/// each over the plain frame in order -- the bands by the reference's band
/// rule, the particles by its particle rule -- in a target `tw` wide whose
/// frame sits at `origin`.
fn effect_frame(
    vfx: &Vfx,
    records: &[Instance],
    plain: &[u32],
    width: Px,
    origin: (usize, usize),
) -> (Vec<u32>, Vec<bool>) {
    effect_frame_by(reference(vfx.effect), vfx, records, plain, width, origin)
}

/// The same, by any rule: the plasma's sheath alone is one. Beside the
/// frame, which of its pixels' last composite landed on a rounding tie
/// ([`on_tie`]), where the blend unit and the reference may round a step
/// apart without either being wrong.
fn effect_frame_by(
    rule: Reference,
    vfx: &Vfx,
    records: &[Instance],
    plain: &[u32],
    width: Px,
    origin: (usize, usize),
) -> (Vec<u32>, Vec<bool>) {
    let tw = usize::try_from(width.value()).unwrap();
    // The frame the records are clipped to, as the GPU's viewport clips
    // them: the target less its margin on each side. A spark thrown past the
    // frame's edge would otherwise land in the margin, or wrap onto the next
    // row of the target.
    let fw = i32::try_from(tw - 2 * origin.0).unwrap();
    let fh = i32::try_from(plain.len() / tw - 2 * origin.1).unwrap();
    let mut out = plain.to_vec();
    let mut ties = vec![false; plain.len()];
    for r in records {
        let (x0, y0) = (i32::from(r.x.0), i32::from(r.y.0));
        let (w, h) = (i32::from(r.w.0), i32::from(r.h.0));
        let color = unit_color(r.color);
        for y in y0.max(0)..(y0 + h).min(fh) {
            for x in x0.max(0)..(x0 + w).min(fw) {
                let at = usize::try_from(y).unwrap() + origin.1;
                let at = at * tw + usize::try_from(x).unwrap() + origin.0;
                let centre = Pt {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                };
                let src = if r.kind == Kind::Particle {
                    let uv = Pt {
                        x: (centre.x - x0 as f32) / w as f32 * 2.0 - 1.0,
                        y: (centre.y - y0 as f32) / h as f32 * 2.0 - 1.0,
                    };
                    (rule.particle)(uv, color)
                } else {
                    (rule.band)(vfx, centre, color)
                };
                ties[at] = on_tie(src, out[at]);
                out[at] = over(src, out[at]);
            }
        }
    }
    (out, ties)
}

/// Whether `src` over `dst` lands within a hundredth of a step of halfway
/// between two bytes in any channel: a tie the blend unit may round either
/// way. A flat patch of one value on a tie rounds apart as a block -- the
/// ufo's hull did, 63 pixels at once.
fn on_tie(src: [f32; 4], dst: u32) -> bool {
    let ch = |k: u32| f32::from(u8::try_from((dst >> k) & 0xff).unwrap_or(0)) / 255.0;
    let (r, g, b, a) = (ch(16), ch(8), ch(0), ch(24));
    let near = |s: f32, d: f32| {
        let v = (s + d * (1.0 - src[3])) * 255.0;
        ((v - v.floor()) - 0.5).abs() < 0.01
    };
    near(src[3], a) || near(src[0], r) || near(src[1], g) || near(src[2], b)
}

/// **Every effect, both arms against its reference**, in a target
/// letterboxed around the frame so the effect's origin is not zero: each
/// effect mid-chase across the screen and one down it, each at four ages,
/// with the body burning and particles born.
///
/// Measured on the fire's reference: at most 9 pixels a scene one step of
/// 255 off, the rest exact -- fast-math `exp` and the blend unit's rounding.
/// One step is the tolerance, and 20 the pixels allowed it.
fn effect_scenes(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    let m = arms.renderer.metrics();
    let (cw, ch) = (m.cell_w.value() as f32, m.cell_h.value() as f32);
    let cell_at = |col: f32, row: f32| Rect {
        left: col * cw,
        top: row * ch,
        right: (col + 1.0) * cw,
        bottom: (row + 1.0) * ch,
    };
    let centre = |r: Rect| Pt {
        x: (r.left + r.right) * 0.5,
        y: (r.top + r.bottom) * 0.5,
    };
    arms.margin = (14, 10);
    let origin = (7usize, 5usize);
    arms.trail = None;
    let (plain, _) = arms.draw(t);
    let (fw, _) = arms.renderer.frame_size();
    let width = Px::of(fw.value() + 14);
    let tw = usize::try_from(width.value()).unwrap();
    let dump = std::env::var("BAUD_FX_DUMP").ok();
    let bar = |r: Rect| Rect {
        top: r.bottom - 2.0,
        ..r
    };
    for effect in Effect::ALL {
        // Each scene: the ignition's move, the move the quad is making, the
        // cursor it arrives at and the unit, in the oracle's cells. The
        // underline scene narrows the cursor to two pixels across a
        // horizontal move; the retarget keeps the ignition's first move
        // while the quad heads somewhere else. The large scene is the move
        // across at two and a half times the unit, a zoomed cell: at the
        // oracle's own the plasma's filaments sit on their 1.1-pixel floor
        // and its width table is never read. The reseeded scene is the move
        // across on a seed whose hashes differ -- the liquid's bead count is
        // two there and three on seed 7 -- and the long one a move far enough
        // for the liquid's head to wobble at a pixel, at full strength. The
        // ridden scene is the move across ridden two and a half rows down a
        // smooth scroll: the only scene whose ignition has ridden, so the
        // only one where a shader that ignored the ride -- the fire's noise
        // anchored on the frame rather than on the path -- differs.
        let across = (cell_at(98.0, 2.0), cell_at(116.0, 3.0));
        let down = (cell_at(104.0, 0.0), cell_at(106.0, 8.0));
        let line = (bar(cell_at(98.0, 4.0)), bar(cell_at(116.0, 4.0)));
        let long = (cell_at(60.0, 2.0), cell_at(116.0, 3.0));
        for (name, (from, to), (quad_from, hole), scale, seed) in [
            ("across", across, across, 1.0f32, Seed(7)),
            ("down", down, down, 1.0, Seed(7)),
            ("underline", line, line, 1.0, Seed(7)),
            (
                "retarget",
                across,
                (across.1, cell_at(104.0, 8.0)),
                1.0,
                Seed(7),
            ),
            ("large", across, across, 2.5, Seed(7)),
            ("reseeded", across, across, 1.0, Seed(1)),
            ("long", long, long, 1.0, Seed(7)),
            ("ridden", across, across, 1.0, Seed(7)),
        ] {
            // The fire's four ages, shrunk for an ignition that is out sooner
            // than the fire's shortest: the lightning's, out at 0.32 s, run
            // from its leader mid-path to its last fade, and a short move's
            // orb its flight to its fade. A longer end keeps the fire's. Then
            // three late in the ignition's own life, which the four never
            // reach for an effect whose last act comes near its end -- the
            // liquid's wobble and fade -- and which ask only agreement.
            let ignite = Ignition::ignite(centre(from), to, seed, ch * scale);
            let end = effect.end(&ignite);
            let life = (end / Effect::Fire.life()).min(1.0);
            let ages = [0.04f32, 0.12, 0.25, 0.4].map(|a| a * life);
            let late = [0.55f32, 0.7, 0.85].map(|a| a * end);
            let mut emitted = false;
            let seg = Segment::new(quad_from.corners(), hole);
            let axis = {
                let (a, b) = (centre(quad_from), centre(hole));
                let l = (b.x - a.x).hypot(b.y - a.y);
                Pt {
                    x: (b.x - a.x) / l,
                    y: (b.y - a.y) / l,
                }
            };
            for (i, age) in ages.into_iter().chain(late).enumerate() {
                let vfx = Vfx {
                    effect,
                    emitter: Smear {
                        corners: seg.at(age),
                        hole,
                        shape: Shape::Quad,
                    },
                    ignition: ignite.at(age),
                    axis,
                };
                let vfx = if name == "ridden" {
                    vfx.translated(2.5 * ch)
                } else {
                    vfx
                };
                arms.trail = Some(TrailLook::Vfx(vfx));
                let (a, b) = arms.draw(t);
                if let Some(dir) = &dump {
                    let bytes: Vec<u8> = a.iter().flat_map(|p| p.to_le_bytes()).collect();
                    std::fs::write(format!("{dir}/{effect:?}-{name}-{age}.bgra"), bytes).unwrap();
                    std::fs::write(format!("{dir}/size"), format!("{tw} {}", a.len() / tw))
                        .unwrap();
                }
                let at = format!("{effect:?} {name} at {age}");
                assert_eq!(differing(&a, &b), 0, "the arms disagree under {at}");
                let records = arms.renderer.trail();
                let (want, ties) = effect_frame(&vfx, records, &plain, width, origin);
                let mut off = 0usize;
                let mut lit = 0usize;
                let inside = |i: usize| {
                    let (x, y) = (i % tw, i / tw);
                    records.iter().any(|r| {
                        let (x0, y0) = (i32::from(r.x.0), i32::from(r.y.0));
                        let (fx, fy) = (
                            i32::try_from(x).unwrap() - i32::try_from(origin.0).unwrap(),
                            i32::try_from(y).unwrap() - i32::try_from(origin.1).unwrap(),
                        );
                        fx >= x0
                            && fx < x0 + i32::from(r.w.0)
                            && fy >= y0
                            && fy < y0 + i32::from(r.h.0)
                    })
                };
                for (i, (&x, &y)) in a.iter().zip(&want).enumerate() {
                    let ch = |p: u32, k: u32| (p >> (8 * k)) & 0xff;
                    let d = (0..4).map(|k| ch(x, k).abs_diff(ch(y, k))).max().unwrap();
                    assert!(d <= 1, "{at}: pixel {i} is {x:08x}, the reference {y:08x}");
                    off += usize::from(d == 1 && !ties[i]);
                    if x != plain[i] {
                        lit += 1;
                        assert!(inside(i), "{at}: pixel {i} changed outside every record");
                    }
                    assert!(
                        (0..3).all(|k| ch(x, k) <= ch(x, 3)),
                        "{at}: pixel {i} {x:08x} stores colour past its alpha"
                    );
                }
                // A pixel on a rounding tie is not counted: the ufo's hull is
                // one flat value, and on a tie the blend unit and the
                // reference round the whole patch apart -- 63 pixels of the
                // zoomed saucer, every one 0x9b against 0x9c. It is still
                // held to one step above.
                assert!(off <= 20, "{at}: {off} pixels a step off the reference");
                if effect == Effect::Plasma {
                    // Nothing but the sheath ahead of the cursor: the
                    // filaments lie on the axis there, and ungated they
                    // poked a spike out of its leading edge.
                    let (sheath, _) = effect_frame_by(
                        Reference {
                            band: plasma_sheath,
                            particle: fx_particle,
                        },
                        &vfx,
                        records,
                        &plain,
                        width,
                        origin,
                    );
                    let mut ahead = 0usize;
                    for (i, (&x, &y)) in a.iter().zip(&sheath).enumerate() {
                        let fx = (i % tw) as f32 - origin.0 as f32 + 0.5;
                        let fy = (i / tw) as f32 - origin.1 as f32 + 0.5;
                        let Some((.., s)) = plasma_parts(&vfx, Pt { x: fx, y: fy }) else {
                            continue;
                        };
                        if s > 0.0 {
                            continue;
                        }
                        ahead += 1;
                        let ch = |p: u32, k: u32| (p >> (8 * k)) & 0xff;
                        let d = (0..4).map(|k| ch(x, k).abs_diff(ch(y, k))).max().unwrap();
                        assert!(
                            d <= 1,
                            "{at}: pixel {i} ahead of the cursor is {x:08x}, the sheath {y:08x}"
                        );
                    }
                    // The control that there was something ahead to check,
                    // where there is: early in the long move kitty's quad is
                    // still short of the cursor, and so is everything the
                    // plasma draws.
                    if i < ages.len() && name != "long" {
                        assert!(ahead > 100, "{at}: only {ahead} pixels ahead of the cursor");
                    }
                }
                // A positive control that something is drawn. At the last age
                // every effect is in its fade -- the plasma starves as
                // (1 - k)^2 -- and only that is asked. The orb's body is out
                // there on a short move: it lands in the cursor, which the
                // hole leaves bare, and is gone 0.25 s after its flight, so
                // what is left is a few dying sparks, 8 pixels on most of
                // these scenes -- which the particle count below holds.
                // A retarget's fire burns on the path the quad left; the ufo
                // is a saucer a couple of cells wide, a wake and a beam, a few
                // hundred pixels at the oracle's cell.
                let least = if age >= ages[3] && effect == Effect::Orb {
                    0
                } else if age >= ages[3] {
                    30
                } else if effect == Effect::Ufo {
                    60
                } else if name == "retarget" {
                    150
                } else {
                    300
                };
                if i < ages.len() {
                    assert!(lit > least, "{at}: only {lit} pixels lit");
                }
                // One particle record for each particle the model sheds at
                // this age, and none where it sheds none.
                let mut shed = 0usize;
                vfx.particles(|_| shed += 1);
                let drawn = records.iter().filter(|r| r.kind == Kind::Particle).count();
                assert_eq!(drawn, shed, "{at}: {drawn} particle records, {shed} shed");
                emitted |= shed > 0;
            }
            // Every effect but the liquid sheds some in every scene: the
            // liquid's beads are part of its field, drawn by its fragment.
            assert!(
                emitted || effect == Effect::Liquid,
                "{effect:?} {name}: no particle at any age"
            );
        }
    }
    arms.trail = None;
    arms.margin = (0, 0);
}

/// The blink animations' phases the oracle draws, part way through each:
/// a fade at half and near its end, the plasma glow, the fire handing
/// over, burning down, in embers and relighting, and the ufo handing over,
/// lifting, holding and setting down.
fn blink_phases() -> Vec<(CursorPhase, &'static str)> {
    let phase = |opacity: u8, glow: u8| CursorPhase {
        opacity: Chan::from_u8(opacity),
        glow: Chan::from_u8(glow),
        ..CursorPhase::HIDDEN
    };
    let fire = |at: u8| CursorPhase::fire(Chan::from_u8(at));
    let ufo = |at: u8| CursorPhase::ufo(Chan::from_u8(at));
    vec![
        (phase(128, 0), "half way through a fade"),
        (phase(40, 0), "near the end of a fade out"),
        (phase(0, 200), "glowing"),
        (fire(10), "handing over to the fire"),
        (fire(70), "burning down"),
        (fire(135), "in embers"),
        (fire(205), "relighting"),
        (ufo(10), "handing over to the ufo"),
        (ufo(70), "lifting"),
        (ufo(128), "held up"),
        (ufo(190), "setting down"),
    ]
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
    // Stock JetBrains Mono's styles -- the embedded Bold is its Bold with
    // Hebrew added, and the scene draws no Hebrew -- and the embedded icon
    // face behind them, so the scene's bold, italic and bold-italic cells
    // are drawn from their own faces on both arms: the cell arm through the
    // style bits in its glyph-map key, the instance arm through the stack.
    // Two arms that both ignored the style would agree, which is why the
    // styled scene carries its own control.
    let app_font = |p: &str| {
        std::fs::read(format!(
            "{}/../baud-app/fonts/{p}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("face")
    };
    let symbols = app_font("symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf");
    let bold = app_font("jetbrains-mono/JetBrainsMono-Bold.ttf");
    let italic = app_font("jetbrains-mono/JetBrainsMono-Italic.ttf");
    let bold_italic = app_font("jetbrains-mono/JetBrainsMono-BoldItalic.ttf");
    let faces = [
        Face::parse(&data).expect("parse"),
        Face::parse(&noto).expect("parse"),
        Face::parse(&symbols).expect("parse"),
        Face::parse(&bold).expect("parse"),
        Face::parse(&italic).expect("parse"),
        Face::parse(&bold_italic).expect("parse"),
    ];
    // The marks row's `q` + U+0316 + U+0301 is the cluster no face draws
    // whole: U+0316 is in none of these, so both arms keep it on JetBrains
    // Mono and drop the mark before shaping (`Renderer::marks_unmapped`).
    // A cluster that moves to another face cannot be built from this stack
    // without a box spilling out of its cell -- the Noto subset has `1` and
    // U+20E3 but not the keycap ligature -- so that path is compared on the
    // CPU, in `baud-render/tests/mark_coverage.rs`.
    for (i, f) in faces.iter().enumerate() {
        assert!(
            f.glyph(baud_domain::Codepoint::new(0x316).unwrap())
                .is_none(),
            "the premise: face {i} lacks U+0316"
        );
    }
    let roles = [
        FaceRole::Fallback,
        FaceRole::Fallback,
        FaceRole::Fallback,
        FaceRole::Style(FaceStyle::Bold),
        FaceRole::Style(FaceStyle::Italic),
        FaceRole::Style(FaceStyle::BoldItalic),
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
        Stack::with_roles(&faces, &roles).unwrap(),
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
        .with_images(ImageBudget::of(64 * 48 + 64));
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
        composition: None,
        copy,
        copy_mem,
        slide: None,
        stamp: StyleStamp::default(),
        margin: (0, 0),
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
        // Four marked cells, and seven ligature pairs under JetBrains
        // Mono, each shaped in a window and drawn by at least one record.
        eprintln!("overlay {name}: {:?}", arms.exceptions.stats());
        assert!(
            arms.exceptions.stats().cells.value() >= 4 + 7,
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
        assert!(
            arms.renderer.marks_unmapped().value() > 0,
            "the dropped-mark cluster reached the drop, {name}"
        );
        let diff = differing(&a, &b);
        let (pw, _) = arms.renderer.frame_size();
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
    // **A blinking cursor's off phase.** Neither arm draws it: the cell arm's
    // cursor goes through `Renderer::cursor_drawn` above, as the app's does,
    // and the instance arm's through `convert`. The arms must agree, the off
    // phase must be exactly the frame `?25l` draws, and it must differ from
    // the on phase -- or a rule lost on both arms at once passes here.
    for (shape, name) in [
        (b"\x1b[1 q", "block"),
        (b"\x1b[3 q", "underline"),
        (b"\x1b[5 q", "bar"),
    ] {
        feed(&mut t, shape);
        let (on_a, _) = arms.draw(&mut t);
        arms.renderer.set_cursor_blink(Blink::Hidden);
        let (off_a, off_b) = arms.draw(&mut t);
        assert_eq!(
            differing(&off_a, &off_b),
            0,
            "the arms disagree on a {name} cursor's off phase"
        );
        assert_ne!(
            differing(&on_a, &off_a),
            0,
            "a {name} cursor's off phase reached no pixel"
        );
        arms.renderer.set_cursor_blink(Blink::Shown);
        feed(&mut t, b"\x1b[?25l");
        let (none_a, _) = arms.draw(&mut t);
        assert_eq!(
            differing(&off_a, &none_a),
            0,
            "a {name} cursor's off phase is not the frame without a cursor"
        );
        feed(&mut t, b"\x1b[?25h");
        // **Part way through a fade, and the plasma glow.** A block at half
        // opacity is each colour half way to its swap -- `mix_level` in the
        // shader, `Argb::mix` on the instance arm -- and a bar or underline
        // is its shape at half alpha; the glow is solids over the cell. The
        // arms must agree to the bit, and each frame must be neither phase,
        // or a fade or glow lost on both arms passes here.
        for (p, what) in blink_phases() {
            arms.renderer.set_cursor_blink(p);
            let (a, b) = arms.draw(&mut t);
            assert_eq!(
                differing(&a, &b),
                0,
                "the arms disagree on a {name} cursor {what}"
            );
            assert_ne!(
                differing(&a, &on_a),
                0,
                "a {name} cursor {what} is the on phase"
            );
            assert_ne!(
                differing(&a, &off_a),
                0,
                "a {name} cursor {what} is the off phase"
            );
        }
        arms.renderer.set_cursor_blink(Blink::Shown);
    }
    // **The same phases over a cell the overlay draws as a record**: the
    // `e` and its acute on the marks row, where the cell arm's glyph is a
    // record and not the map's. The loop above sits on a map-drawn `H`, so
    // a record that missed the fade's mix or the effect's solids would
    // pass there. The positive control is the mask: the cell is the
    // overlay's.
    feed(&mut t, b"\x1b[7;7H");
    // An `OSC 12` bar crosses the glyph: drawn under the record's glyph on
    // the cell arm and over it on the instance arm, ten pixels differed.
    for (shape, name) in [(b"\x1b[6 q", "bar"), (b"\x1b[4 q", "underline")] {
        feed(&mut t, shape);
        feed(&mut t, b"\x1b]12;rgb:ff/00/00\x07");
        let (a, b) = arms.draw(&mut t);
        assert_eq!(
            differing(&a, &b),
            0,
            "the arms disagree on an OSC 12 {name} over a record"
        );
        feed(&mut t, b"\x1b]112\x07");
    }
    let cols = usize::from(WS.ws_col);
    for (shape, name) in [
        (b"\x1b[1 q", "block"),
        (b"\x1b[3 q", "underline"),
        (b"\x1b[5 q", "bar"),
    ] {
        feed(&mut t, shape);
        let (on_a, _) = arms.draw(&mut t);
        assert_ne!(
            arms.overlay_mem[6 * cols + 6],
            0,
            "the control: the marked cell is drawn by a record"
        );
        arms.renderer.set_cursor_blink(Blink::Hidden);
        let (off_a, _) = arms.draw(&mut t);
        for (p, what) in blink_phases() {
            arms.renderer.set_cursor_blink(p);
            let (a, b) = arms.draw(&mut t);
            assert_eq!(
                differing(&a, &b),
                0,
                "the arms disagree on a {name} cursor {what} over a record"
            );
            assert_ne!(differing(&a, &on_a), 0, "{name} {what}: the on phase");
            assert_ne!(differing(&a, &off_a), 0, "{name} {what}: the off phase");
        }
        arms.renderer.set_cursor_blink(Blink::Shown);
    }
    feed(&mut t, b"\x1b[1;1H");
    // **Text that blinks, in its off phase, under a block cursor and a
    // selection.** The cursor block and the selection block are drawn; the
    // blinking glyph, its underline and its strike are not. So the blink's
    // hide comes *after* the swaps: hidden first, the glyph takes the cell's
    // background and the swap then paints the block in it, and the cursor
    // disappears into the text around it.
    feed(
        &mut t,
        "\x1b[2;60H\x1b[5mBLINK\x1b[0m \x1b[5;4;9;31mul-st\x1b[0m \x1b[5me\u{301}->\x1b[0m\x1b[2 q\x1b[2;62H"
            .as_bytes(),
    );
    arms.renderer.set_blink(BlinkPhases {
        slow: Blink::Hidden,
        rapid: Blink::Shown,
    });
    let (off_a, off_b) = arms.draw(&mut t);
    {
        let (w, _) = arms.renderer.frame_size();
        let m = arms.renderer.metrics();
        let cell = (
            usize::try_from(m.cell_w.value()).unwrap(),
            usize::try_from(m.cell_h.value()).unwrap(),
        );
        let bx = diff_box(&off_a, &off_b, usize::try_from(w.value()).unwrap(), cell);
        assert_eq!(
            differing(&off_a, &off_b),
            0,
            "the arms disagree on blinking text's off phase under a block cursor, in cells {bx:?}"
        );
    }
    feed(&mut t, b"\x1b[?25l");
    let (none_a, none_b) = arms.draw(&mut t);
    assert_ne!(
        differing(&off_a, &none_a),
        0,
        "the block cursor over blinking text vanished in the off phase"
    );
    assert_eq!(differing(&none_a, &none_b), 0);
    feed(&mut t, b"\x1b[?25h");
    arms.highlight = Some(Highlight {
        from: (Row::from_index(1), Col::at_index(58)),
        to: (Row::from_index(1), Col::at_index(70)),
        region: Region::Stream,
    });
    let (sel_a, sel_b) = arms.draw(&mut t);
    assert_eq!(
        differing(&sel_a, &sel_b),
        0,
        "the arms disagree on blinking text's off phase under a selection"
    );
    arms.highlight = None;
    // And on each arm the off phase is the frame of spaces in the text's
    // background -- here the default's, translucent -- with the cursor where
    // it was: a glyph drawn in its background's colour at another alpha
    // leaves its silhouette, which agreeing arms would share.
    feed(&mut t, b"\x1b[2;60H               \x1b[2;62H");
    let (sp_a, sp_b) = arms.draw(&mut t);
    assert_eq!(
        differing(&off_a, &sp_a),
        0,
        "the instance arm drew something of blinking text in its off phase"
    );
    assert_eq!(
        differing(&off_b, &sp_b),
        0,
        "the cell arm drew something of blinking text in its off phase"
    );
    feed(
        &mut t,
        "\x1b[2;60H\x1b[5mBLINK\x1b[0m \x1b[5;4;9;31mul-st\x1b[0m \x1b[5me\u{301}->\x1b[0m\x1b[2;62H"
            .as_bytes(),
    );
    // A bar cursor over blinking text in its off phase is still drawn: the
    // cursor's shape is not the cell's text.
    feed(&mut t, b"\x1b[6 q");
    let (bar_a, bar_b) = arms.draw(&mut t);
    assert_eq!(
        differing(&bar_a, &bar_b),
        0,
        "the arms disagree on a bar cursor over blinking text's off phase"
    );
    feed(&mut t, b"\x1b[?25l");
    let (nobar_a, _) = arms.draw(&mut t);
    assert_ne!(
        differing(&bar_a, &nobar_a),
        0,
        "the bar cursor over blinking text vanished in the off phase"
    );
    feed(&mut t, b"\x1b[?25h\x1b[2 q");
    arms.renderer.set_blink(BlinkPhases::SHOWN);
    let (on_a, _) = arms.draw(&mut t);
    assert_ne!(
        differing(&on_a, &off_a),
        0,
        "blinking text's off phase reached no pixel"
    );
    // **Each rate in its own phase.** Slow text (`SGR 5`) and rapid text
    // (`SGR 6`), with an underline and a strike, side by side: in each mixed
    // phase the arms agree, and each is the frame with only the hidden
    // rate's text replaced by its spaces. A shader that read one phase for
    // both rates hides the wrong word.
    feed(&mut t, b"\x1b[?25l");
    let words = b"\x1b[3;60H\x1b[5;4;9;31mSLOW\x1b[0m \x1b[6;4;9;32mFAST\x1b[0m";
    for (phase, gap, why) in [
        (
            BlinkPhases {
                slow: Blink::Hidden,
                rapid: Blink::Shown,
            },
            &b"\x1b[3;60H    \x1b[0m \x1b[6;4;9;32mFAST\x1b[0m"[..],
            "the slow off phase",
        ),
        (
            BlinkPhases {
                slow: Blink::Shown,
                rapid: Blink::Hidden,
            },
            &b"\x1b[3;60H\x1b[5;4;9;31mSLOW\x1b[0m     "[..],
            "the rapid off phase",
        ),
    ] {
        arms.renderer.set_blink(phase);
        feed(&mut t, words);
        let (a, b) = arms.draw(&mut t);
        assert_eq!(differing(&a, &b), 0, "the arms disagree on {why}");
        feed(&mut t, b"\x1b[3;60H\x1b[K");
        feed(&mut t, gap);
        let (ga, gb) = arms.draw(&mut t);
        assert_eq!(differing(&a, &ga), 0, "the instance arm in {why}");
        assert_eq!(differing(&b, &gb), 0, "the cell arm in {why}");
        feed(&mut t, b"\x1b[3;60H\x1b[K");
    }
    arms.renderer.set_blink(BlinkPhases::SHOWN);
    feed(&mut t, words);
    let (both_a, _) = arms.draw(&mut t);
    feed(&mut t, b"\x1b[3;60H\x1b[K");
    let (blank_a, _) = arms.draw(&mut t);
    assert_ne!(
        differing(&both_a, &blank_a),
        0,
        "the control: the words draw"
    );
    feed(&mut t, b"\x1b[?25h");
    // `SGR 8` the same way, in either phase: concealed text with an underline
    // and a strike is the frame of its spaces, on each arm.
    feed(
        &mut t,
        "\x1b[2;60H\x1b[8;4;9;31mconceal e\u{301}->\x1b[0m    \x1b[?25l".as_bytes(),
    );
    let (hid_a, hid_b) = arms.draw(&mut t);
    assert_eq!(
        differing(&hid_a, &hid_b),
        0,
        "the arms disagree on concealed text"
    );
    feed(&mut t, b"\x1b[2;60H               ");
    let (sp_a, sp_b) = arms.draw(&mut t);
    assert_eq!(
        differing(&hid_a, &sp_a),
        0,
        "the instance arm drew something of concealed text"
    );
    assert_eq!(
        differing(&hid_b, &sp_b),
        0,
        "the cell arm drew something of concealed text"
    );
    feed(&mut t, b"\x1b[?25h\x1b[2;60H\x1b[K");
    // Back to the steady bar the `OSC 12` loop left, which is the cursor
    // the cases below were written against.
    feed(&mut t, b"\x1b[6 q");
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
    // it: solids on the instance list and on the cell path's records, equal
    // between the arms and a change to the pixels -- held at full, halfway
    // through its fade, and widened over a track under the pointer. The
    // widths are AppKit's at 2x, measured.
    let widths = Widths {
        rest: Px::of(12),
        hovered: Px::of(22),
        inset: Px::of(6),
        strip: Px::of(34),
    };
    let window = Window {
        retained: Rows::of_count(t.scrollback().len()),
        back: Rows::of_count(3),
        rows: t.viewport().rows(),
    };
    let scroll = |shown: u8, hover: Hover| Scroll {
        window,
        look: Look {
            shown: Chan::from_u8(shown),
            hover,
        },
        widths,
        scroller: Scroller::Overlay,
    };
    let mut frames = Vec::new();
    for (shown, hover, what) in [
        (255, Hover::Rest, "held"),
        (128, Hover::Rest, "halfway faded"),
        (255, Hover::Hovered, "hovered, with its track"),
    ] {
        arms.window = Some(scroll(shown, hover));
        let (bar_a, bar_b) = arms.draw(&mut t);
        assert_eq!(
            differing(&bar_a, &bar_b),
            0,
            "the arms disagree with a scrollbar {what}"
        );
        assert!(
            differing(&bar_a, &a) > 0,
            "the thumb {what} reached the pixels"
        );
        frames.push(bar_a);
    }
    // Three looks, three pictures: a fade and a hover are seen, not just
    // drawn.
    assert!(
        differing(&frames[0], &frames[1]) > 0,
        "the fade changed nothing"
    );
    assert!(
        differing(&frames[0], &frames[2]) > 0,
        "the hover changed nothing"
    );
    // A thumb faded to nothing is not a scene here: its solids would carry
    // no alpha, so no pixel could tell it from the frame without it. The
    // scrollbar's geometry test holds that no bar is built at all.
    //
    // The legacy scroller, in a gutter of its own beside the grid: the frame
    // is the grid and the strip, and both arms letterbox the frame. The arms
    // agreeing cannot show the gutter is drawn -- both could lose it
    // together -- so the pixels are read: on every row the gutter's margin
    // columns are what they are with no bar at all, its track columns are
    // not, and the grid is untouched by the bar.
    arms.renderer.set_gutter(widths.strip);
    arms.window = None;
    let (bare, bare_b) = arms.draw(&mut t);
    assert_eq!(
        differing(&bare, &bare_b),
        0,
        "the arms disagree about an empty gutter"
    );
    arms.window = Some(Scroll {
        scroller: Scroller::Legacy,
        ..scroll(0, Hover::Rest)
    });
    let (legacy_a, legacy_b) = arms.draw(&mut t);
    let (fw, fh) = arms.renderer.frame_size();
    let (gw, _) = arms.renderer.pixel_size();
    let (fw, fh, gw) = (
        usize::try_from(fw.value()).unwrap(),
        usize::try_from(fh.value()).unwrap(),
        usize::try_from(gw.value()).unwrap(),
    );
    assert_eq!(
        differing(&legacy_a, &legacy_b),
        0,
        "the arms disagree with a legacy scroller at cells {:?}",
        diff_box(&legacy_a, &legacy_b, fw, (8, 18))
    );
    assert_eq!(fw - gw, 34, "the frame is the grid and the gutter");
    let track = (fw - 6 - 22)..(fw - 6);
    for y in 0..fh {
        let (row, none) = (&legacy_a[y * fw..(y + 1) * fw], &bare[y * fw..(y + 1) * fw]);
        assert_eq!(&row[..gw], &none[..gw], "row {y}: the bar reached a cell");
        for x in gw..fw {
            match track.contains(&x) {
                true => assert_ne!(row[x], none[x], "row {y} column {x}: no track"),
                false => assert_eq!(row[x], none[x], "row {y} column {x}: not the margin"),
            }
        }
    }
    arms.renderer.set_gutter(Px::of(0));
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
        let (w, _) = arms.renderer.frame_size();
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
        let (w, _) = arms.renderer.frame_size();
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
    // **The trails' own baseline, drawn over the screen as it now is.** The
    // `plain` above predates the two watches written for the block, so a
    // trail compared against it differed at the watches whether or not the
    // trail drew anything: the ghost's "reached the pixels" could not fail.
    let (plain, _) = arms.draw(&mut t);
    // **An input method's composition**, at the cursor and over the text
    // there: a narrow run and the wide U+231A, which the Noto subset draws.
    // The cell arm masks the cells and draws records; the instance arm
    // converts them blank and draws the same records over the row. Both
    // put the cursor after it, and both must change the pixels.
    let (cr, cc) = t.cursor();
    arms.composition = Some(Composition {
        row: cr,
        col: cc,
        text: "ime\u{231a}"
            .chars()
            .map(|c| baud_domain::Codepoint::new(u32::from(c)).unwrap())
            .collect(),
        ambiguous: baud_domain::AmbiguousWidth::Narrow,
    });
    let (comp_a, comp_b) = arms.draw(&mut t);
    assert_eq!(
        differing(&comp_a, &comp_b),
        0,
        "the arms disagree over an input method's composition"
    );
    assert!(
        differing(&comp_a, &plain) > 0,
        "the composition reached the pixels"
    );
    let comp_c = arms.draw_copied(&mut t);
    assert_eq!(
        differing(&comp_b, &comp_c),
        0,
        "the copy disagrees with the live grid under a composition"
    );
    arms.composition = None;
    // The cursor trail: a fading ghost mid-sweep between two cells, drawn
    // by both arms -- one translucent solid on the instance list and on the
    // cell path's records -- and a change to the pixels. `plain` has the
    // same cursor and no trail, so the difference is the ghost alone.
    arms.trail = Some(TrailLook::Ghost(Trail {
        from: (Row::from_index(1), Col::at_index(2)),
        to: (Row::from_index(4), Col::at_index(20)),
        progress: 0.5,
        origin_dy: 0.0,
        lift: Px::of(0),
    }));
    let (trail_a, trail_b) = arms.draw(&mut t);
    {
        let (w, _) = arms.renderer.frame_size();
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
    // The quad trail, 50 ms into a move from row 7 to the block cursor at
    // row 1, column 2: both arms draw it -- the record's bounds, cut by
    // `quad_fragment` -- and neither touches the cursor it is arriving at.
    let (fw, _) = arms.renderer.frame_size();
    let fw = usize::try_from(fw.value()).unwrap();
    let (cr, cc) = t.cursor();
    let hole = arms.renderer.cursor_rect(cr, cc, t.cursor_style());
    let from = arms
        .renderer
        .cursor_rect(Row::from_index(7), Col::at_index(60), t.cursor_style())
        .corners();
    let seg = Segment::new(from, hole);
    arms.trail = Some(TrailLook::Smear(Smear {
        corners: seg.at(0.05),
        hole,
        shape: Shape::Quad,
    }));
    let (quad_a, quad_b) = arms.draw(&mut t);
    assert_eq!(
        differing(&quad_a, &quad_b),
        0,
        "the arms disagree under a quad trail"
    );
    assert!(
        differing(&quad_a, &plain) > 200,
        "the quad reached the pixels"
    );
    // At 200 ms the quad has reached the cursor and lies over it: drawn with
    // the cursor's rectangle as its hole it leaves the cursor as it was, and
    // drawn with no hole it covers it -- so the hole is what spared it.
    let px = |v: f32| usize::try_from(v as i64).unwrap();
    let in_hole = |frame: &[u32]| -> Vec<u32> {
        let mut out = Vec::new();
        for y in px(hole.top)..px(hole.bottom) {
            for x in px(hole.left)..px(hole.right) {
                out.push(frame[y * fw + x]);
            }
        }
        out
    };
    arms.trail = Some(TrailLook::Smear(Smear {
        corners: seg.at(0.2),
        hole,
        shape: Shape::Quad,
    }));
    let (spared, spared_b) = arms.draw(&mut t);
    assert_eq!(
        differing(&spared, &spared_b),
        0,
        "the arms disagree over the cursor"
    );
    assert_eq!(
        in_hole(&spared),
        in_hole(&plain),
        "the quad covered the cursor"
    );
    arms.trail = Some(TrailLook::Smear(Smear {
        corners: seg.at(0.2),
        hole: Rect::default(),
        shape: Shape::Quad,
    }));
    let (covered, _) = arms.draw(&mut t);
    assert!(
        differing(&in_hole(&covered), &in_hole(&plain)) > 0,
        "the quad does not reach the cursor, so the hole spared nothing"
    );
    // A dart and a bowtie -- the shapes a retarget can twist the quad into,
    // which a triangle fan draws wrong -- over the empty right half of the
    // screen. Every pixel the Rust rule covers whole is drawn and every one
    // it leaves bare is not, so the shader's rule is `trail::coverage`.
    let m = arms.renderer.metrics();
    let (cw, ch) = (m.cell_w.value() as f32, m.cell_h.value() as f32);
    let no_hole = Rect::default();
    for (name, pts) in [
        (
            "dart",
            [
                (100.0 * cw, 0.5 * ch),
                (100.0 * cw, 6.5 * ch),
                (70.0 * cw, 6.5 * ch),
                (95.0 * cw, 5.0 * ch),
            ],
        ),
        (
            "bowtie",
            [
                (118.0 * cw, 0.5 * ch),
                (118.0 * cw, 6.5 * ch),
                (104.0 * cw, 0.5 * ch),
                (104.0 * cw, 6.5 * ch),
            ],
        ),
    ] {
        let smear = Smear {
            corners: Corners(pts.map(|(x, y)| Pt { x, y })),
            hole: no_hole,
            shape: Shape::Quad,
        };
        arms.trail = Some(TrailLook::Smear(smear));
        let (a, b) = arms.draw(&mut t);
        assert_eq!(differing(&a, &b), 0, "the arms disagree under a {name}");
        let (x0, y0, x1, y1) = smear.bounds();
        let (mut inside, mut outside) = (0usize, 0usize);
        for y in y0.value()..y1.value() {
            for x in x0.value()..x1.value() {
                let at = usize::try_from(y).unwrap() * fw + usize::try_from(x).unwrap();
                let c = quad_coverage(
                    &smear,
                    Pt {
                        x: x as f32 + 0.5,
                        y: y as f32 + 0.5,
                    },
                );
                if c >= 1.0 {
                    inside += 1;
                    assert_ne!(
                        a[at], plain[at],
                        "{name}: ({x}, {y}) is inside and not drawn"
                    );
                } else if c <= 0.0 {
                    outside += 1;
                    assert_eq!(a[at], plain[at], "{name}: ({x}, {y}) is outside and drawn");
                }
            }
        }
        assert!(
            inside > 500 && outside > 500,
            "{name}: {inside} in, {outside} out"
        );
    }
    // sfte's sweep, over the same empty half, twice: once mostly across,
    // where the band's height is what bounds it, and once mostly down,
    // where its width is. Every pixel away from its hard edge that the Rust
    // rule draws at more than a few steps of 255 is drawn, every one it
    // leaves bare -- the cursor's own cell among them -- is not, and the
    // drawing darkens toward the cursor, so the shader's ramp is
    // `trail::coverage`'s. One diagonal is not enough: a band test that
    // lost its test on one axis draws the same as long as the other binds.
    let cell_at = |col: f32, row: f32| Rect {
        left: col * cw,
        top: row * ch,
        right: (col + 1.0) * cw,
        bottom: (row + 1.0) * ch,
    };
    let alpha = |smear: &Smear, x: u32, y: u32| {
        quad_coverage(
            smear,
            Pt {
                x: x as f32 + 0.5,
                y: y as f32 + 0.5,
            },
        ) * 255.0
    };
    let delta = |p: u32, q: u32| -> u32 {
        (0..3)
            .map(|k| ((p >> (8 * k)) & 0xff).abs_diff((q >> (8 * k)) & 0xff))
            .sum()
    };
    for (name, tail, head) in [
        ("across", cell_at(100.0, 1.0), cell_at(118.0, 3.0)),
        ("down", cell_at(102.0, 0.0), cell_at(104.0, 6.0)),
    ] {
        let smear = Smear {
            corners: tail.corners(),
            hole: head,
            shape: Shape::Sweep,
        };
        arms.trail = Some(TrailLook::Smear(smear));
        let (a, b) = arms.draw(&mut t);
        assert_eq!(
            differing(&a, &b),
            0,
            "the arms disagree under a sweep {name}"
        );
        let (x0, y0, x1, y1) = smear.bounds();
        let (mut inside, mut outside, mut hole_px) = (0usize, 0usize, 0usize);
        let (mut faint, mut strong) = ((0u32, 0u32), (0u32, 0u32));
        for y in y0.value() + 1..y1.value() - 1 {
            for x in x0.value() + 1..x1.value() - 1 {
                let c = alpha(&smear, x, y);
                let near = [
                    alpha(&smear, x - 1, y),
                    alpha(&smear, x + 1, y),
                    alpha(&smear, x, y - 1),
                    alpha(&smear, x, y + 1),
                ];
                if near.iter().any(|&n| (n > 0.0) != (c > 0.0)) {
                    continue;
                }
                let at = usize::try_from(y).unwrap() * fw + usize::try_from(x).unwrap();
                if c >= 4.0 {
                    inside += 1;
                    assert_ne!(
                        a[at], plain[at],
                        "sweep {name}: ({x}, {y}) is inside and not drawn"
                    );
                    let d = delta(a[at], plain[at]);
                    if (16.0..40.0).contains(&c) {
                        faint = (faint.0 + d, faint.1 + 1);
                    } else if c >= 100.0 {
                        strong = (strong.0 + d, strong.1 + 1);
                    }
                } else if c <= 0.0 {
                    outside += 1;
                    if head.contains(Pt {
                        x: x as f32 + 0.5,
                        y: y as f32 + 0.5,
                    }) {
                        hole_px += 1;
                    }
                    assert_eq!(
                        a[at], plain[at],
                        "sweep {name}: ({x}, {y}) is outside and drawn"
                    );
                }
            }
        }
        assert!(
            inside > 500 && outside > 500 && hole_px as f32 * 2.0 > cw * ch,
            "sweep {name}: {inside} in, {outside} out, {hole_px} in the cursor's cell"
        );
        assert!(
            faint.1 > 20 && strong.1 > 20 && faint.0 * strong.1 < strong.0 * faint.1,
            "sweep {name}: the faint end differs by {faint:?} and the strong by {strong:?}"
        );
    }
    effect_scenes(&mut arms, &mut t);
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
    // `?5`, reverse video: every rendition resolves exchanged, the
    // default background at the window's dim like any other. Both arms draw
    // the flip from the one resolved table, the fade over it takes the
    // reversed background, and the frame is not the one before it.
    feed(&mut t, b"\x1b[?5h");
    let (rev_a, rev_b) = arms.draw(&mut t);
    assert_eq!(
        differing(&rev_a, &rev_b),
        0,
        "the arms disagree under reverse video"
    );
    assert!(
        differing(&rev_a, &unveiled) > 0,
        "reverse video reached the pixels"
    );
    arms.fade = Some(Transition { progress: 0.5 });
    let (rev_fade_a, rev_fade_b) = arms.draw(&mut t);
    assert_eq!(
        differing(&rev_fade_a, &rev_fade_b),
        0,
        "the arms disagree under a screen fade in reverse video"
    );
    arms.fade = None;
    feed(&mut t, b"\x1b[?5l");
    let (back_a, _) = arms.draw(&mut t);
    assert_eq!(
        differing(&back_a, &unveiled),
        0,
        "?5 l draws the frame before the flip"
    );
    // A visual bell no frame drew: the other video with `?5` untouched.
    // Both arms draw it from the one resolved table, and it is the frame
    // `?5 h` drew, pixel for pixel -- one rule, not a second spelling.
    arms.renderer.set_flash(VideoFlash::On);
    let (flash_a, flash_b) = arms.draw(&mut t);
    assert_eq!(
        differing(&flash_a, &flash_b),
        0,
        "the arms disagree under a flash"
    );
    assert_eq!(
        differing(&flash_a, &rev_a),
        0,
        "a flash draws what reverse video draws"
    );
    // **Again with an `OSC 12` cursor colour equal to the text**, and the
    // theme's colours told to the core as the app tells them: the flash
    // draws the colour one flip would leave, the page, on both arms -- a
    // cell arm that read the stored colour instead of `cursor_color_in`
    // would draw the text and disagree with the instance arm -- and it is
    // the frame a real `?5 h` then draws with the colour the flip moved.
    t.set_default_colors((0xd8, 0xd8, 0xd8), (0x10, 0x10, 0x10));
    feed(&mut t, b"\x1b]12;rgb:d8/d8/d8\x07");
    let (flash_a, flash_b) = arms.draw(&mut t);
    assert_eq!(
        differing(&flash_a, &flash_b),
        0,
        "the arms disagree under a flash with an OSC 12 equal to the text"
    );
    arms.renderer.set_flash(VideoFlash::Off);
    feed(&mut t, b"\x1b[?5h");
    let (moved_a, _) = arms.draw(&mut t);
    assert_eq!(
        differing(&flash_a, &moved_a),
        0,
        "a flash draws the cursor colour ?5 h moves it to"
    );
    feed(&mut t, b"\x1b[?5l\x1b]112\x07");
    // The positive control: moving the cursor makes the arms disagree, so
    // an equal pair above is two identical drawings and not two blanks.
    let (a, _) = arms.draw(&mut t);
    feed(&mut t, b"\x1b[1;1H");
    let (_, b) = arms.draw(&mut t);
    assert!(differing(&a, &b) > 0, "the comparison can fail");

    // **A composition over a ligature**: JetBrains Mono's `->`, covered
    // whole and in part. The instance arm blanks the covered cells before
    // it shapes, so the pair is gone and an uncovered half draws alone; the
    // cell arm's window walk must not have drawn the pair already, or its
    // glyph stays under the composition's. Each is held to the text written
    // under `SGR 4` with the cursor where the composition leaves it.
    let (cr, cc) = t.cursor();
    for (at, text, written) in [
        (14, "zz", &b"xyyy\x1b[4mzz\x1b[24mb\x1b[6;16H"[..]),
        (14, "z", &b"xyyy\x1b[4mz\x1b[24m>b\x1b[6;15H"[..]),
        (13, "zz", &b"xyy\x1b[4mzz\x1b[24m>b\x1b[6;15H"[..]),
    ] {
        feed(&mut t, b"\x1b[6;10H\x1b[Kxyyy->b");
        feed(&mut t, format!("\x1b[6;{at}H").as_bytes());
        let (row, col) = t.cursor();
        arms.composition = Some(Composition {
            row,
            col,
            text: text
                .chars()
                .map(|c| baud_domain::Codepoint::new(u32::from(c)).unwrap())
                .collect(),
            ambiguous: baud_domain::AmbiguousWidth::Narrow,
        });
        // `draw` returns the instance arm first and the cell arm second.
        let (inst, cell) = arms.draw(&mut t);
        let copied = arms.draw_copied(&mut t);
        arms.composition = None;
        feed(&mut t, b"\x1b[6;10H\x1b[K");
        feed(&mut t, written);
        let (ref_inst, ref_cell) = arms.draw(&mut t);
        assert_eq!(
            differing(&ref_inst, &ref_cell),
            0,
            "{text:?} at {at}: the arms disagree over the written text"
        );
        assert_eq!(
            differing(&inst, &ref_inst),
            0,
            "{text:?} at {at}: the instance arm's composition is not the written text"
        );
        assert_eq!(
            differing(&cell, &inst),
            0,
            "{text:?} at {at}: the cell arm draws the ligature under the composition"
        );
        assert_eq!(
            differing(&copied, &cell),
            0,
            "{text:?} at {at}: the copy disagrees under a composition over a ligature"
        );
    }
    // A marked cell under a composition: the walk leaves it to the
    // composition, or its mark's records stay under the composition's glyph.
    {
        feed(&mut t, "\x1b[6;10H\x1b[Kxe\u{301}y\x1b[6;11H".as_bytes());
        let (row, col) = t.cursor();
        arms.composition = Some(Composition {
            row,
            col,
            text: vec![baud_domain::Codepoint::new(u32::from('z')).unwrap()],
            ambiguous: baud_domain::AmbiguousWidth::Narrow,
        });
        let (inst, cell) = arms.draw(&mut t);
        arms.composition = None;
        feed(&mut t, b"\x1b[6;10H\x1b[K");
        feed(&mut t, b"x\x1b[4mz\x1b[24my\x1b[6;12H");
        let (ref_inst, _) = arms.draw(&mut t);
        assert_eq!(
            differing(&inst, &ref_inst),
            0,
            "the instance arm's composition over a mark is not the written text"
        );
        assert_eq!(
            differing(&cell, &inst),
            0,
            "the cell arm draws a mark under the composition"
        );
    }
    feed(&mut t, b"\x1b[6;10H\x1b[K");
    feed(
        &mut t,
        format!("\x1b[{};{}H", cr.index() + 1, cc.index() + 1).as_bytes(),
    );

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
    arms.renderer
        .sync_styles(&t, arms.styles_mem, &mut arms.stamp);
    arms.map.ensure(
        &t,
        t.view(Rows::of_count(1)),
        arms.styles_mem,
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

    // A marked cell on the leaving row draws its base there, the row the
    // shader draws from the map alone, with no overlay mask -- as it drew
    // when the base was the cell's content. The cell holds its cluster's
    // slot now, above U+10FFFF, which the shader draws nothing for, so the
    // copy the app binds gives it its base (`copy_for_shader`). Against the
    // same screen with the mark never sent, where the cell holds the bare
    // letter, a full-row slide over the copy is the same frame; and against
    // another letter there it is not, so the leaving row's letter reached
    // the pixels. The letter is `Q`, which nothing before this draws, so the
    // map holds it only if the walk over the leaving row gave the marked
    // cell's base an entry.
    let mut leaving = |t: &mut Term<'_>, first: &str| {
        feed(t, b"\x1b[2J\x1b[H");
        for i in 0..14u32 {
            let lead = if i == 4 { first } else { "0" };
            feed(t, format!("{lead}000 1111 row {i:02} 2222\r\n").as_bytes());
        }
        arms.renderer
            .sync_styles(t, arms.styles_mem, &mut arms.stamp);
        arms.map.ensure(
            t,
            t.view(Rows::of_count(1)),
            arms.styles_mem,
            &mut arms.renderer,
            &mut arms.candidates,
        );
        let above = t.view(Rows::of_count(1)).row(Row::ZERO)[0];
        arms.back = Rows::ZERO;
        arms.slide = Some(cell_h);
        arms.draw(t);
        let frame = arms.draw_copied(t);
        arms.slide = None;
        (above, frame)
    };
    let (marked, with_mark) = leaving(&mut t, "Q\u{0301}");
    let (bare, without) = leaving(&mut t, "Q");
    let (_, other) = leaving(&mut t, "x");
    assert!(
        marked.flags.is_marked() && !bare.flags.is_marked(),
        "the premise: the leaving row's first cell carries the mark, then not"
    );
    assert_eq!(
        differing(&with_mark, &without),
        0,
        "a marked cell on the leaving row draws its base"
    );
    assert!(
        differing(&with_mark, &other) > 0,
        "the leaving row's letter reached the pixels"
    );
    // The digit screen again, for the ride-scroll gate below.
    feed(&mut t, b"\x1b[2J\x1b[H");
    for i in 0..14u32 {
        feed(
            &mut t,
            format!("0000 1111 2222 row {i:02} 3333 4444 5555\r\n").as_bytes(),
        );
    }
    arms.renderer
        .sync_styles(&t, arms.styles_mem, &mut arms.stamp);
    arms.map.ensure(
        &t,
        t.view(Rows::of_count(1)),
        arms.styles_mem,
        &mut arms.renderer,
        &mut arms.candidates,
    );

    // Ride-scroll: a trail drawn mid-slide is lifted by the slide's pan,
    // which the cell path pans the grid and the cursor by too, so the frame
    // is the resting frame moved down by the pan, trail and all -- pixel for
    // pixel wherever both frames draw, below the leaving row. A move up
    // from four rows below: the quad is a column under the cursor, so a
    // trail left unlifted covers the lower part of the cursor the slide
    // moved down into it, which is the control that the check can fail.
    feed(&mut t, b"\x1b[?25h\x1b[5;30H");
    let half = Px::of(cell_h.value() / 2);
    let (r, c) = t.cursor();
    let style = t.cursor_style();
    let hole = arms.renderer.cursor_rect(r, c, style);
    let below = arms
        .renderer
        .cursor_rect(Row::from_index(r.index() + 4), c, style);
    let seg = Segment::new(below.corners(), hole);
    let quad = TrailLook::Smear(Smear {
        corners: seg.at(0.03),
        hole,
        shape: Shape::Quad,
    });
    let ignite = Ignition::ignite(
        Pt {
            x: (below.left + below.right) * 0.5,
            y: (below.top + below.bottom) * 0.5,
        },
        hole,
        Seed(7),
        cell_h.value() as f32,
    );
    let fire = TrailLook::Vfx(Vfx {
        effect: Effect::Fire,
        emitter: Smear {
            corners: seg.at(0.1),
            hole,
            shape: Shape::Quad,
        },
        ignition: ignite.at(0.1),
        axis: Pt { x: 0.0, y: -1.0 },
    });
    let (fw, _) = arms.renderer.frame_size();
    let w = usize::try_from(fw.value()).unwrap();
    let shift = usize::try_from(half.value()).unwrap() * w;
    // Pixels of the slid frame that are not the resting frame moved down.
    let moved = |slid: &[u32], rest: &[u32]| {
        (0..rest.len() - shift)
            .filter(|&i| slid[i + shift] != rest[i])
            .count()
    };
    // The same, only over the cursor as the slide draws it.
    let in_cursor = |slid: &[u32], rest: &[u32]| {
        (0..rest.len() - shift)
            .filter(|&i| {
                let (x, y) = ((i % w) as f32 + 0.5, (i / w) as f32 + 0.5);
                x > hole.left && x < hole.right && y > hole.top && y < hole.bottom
            })
            .filter(|&i| slid[i + shift] != rest[i])
            .count()
    };
    arms.back = Rows::ZERO;
    for (name, look) in [
        ("nothing", None),
        ("quad", Some(quad)),
        ("fire", Some(fire)),
    ] {
        arms.slide = None;
        arms.trail = look;
        let (_, rest) = arms.draw(&mut t);
        arms.slide = Some(half);
        arms.trail = look.map(|l| l.lifted(half));
        let (_, slid) = arms.draw(&mut t);
        assert_eq!(
            moved(&slid, &rest),
            0,
            "mid-slide, the {name} is not the resting frame moved by the pan"
        );
        if look.is_some() {
            arms.trail = None;
            let (_, plain_slid) = arms.draw(&mut t);
            assert!(
                differing(&slid, &plain_slid) > 100,
                "the {name} reached the pixels mid-slide"
            );
        }
    }
    arms.trail = None;
    arms.slide = None;
    let (_, rest) = arms.draw(&mut t);
    arms.slide = Some(half);
    arms.trail = Some(quad);
    let (_, unlifted) = arms.draw(&mut t);
    assert!(
        in_cursor(&unlifted, &rest) > 0,
        "an unlifted quad covers the cursor the slide moved into it"
    );
    arms.trail = None;
    arms.slide = None;
    feed(&mut t, b"\x1b[?25l");

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
    arms.renderer
        .sync_styles(&t, arms.styles_mem, &mut arms.stamp);
    arms.map.ensure(
        &t,
        t.view(Rows::of_count(1)),
        arms.styles_mem,
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
    let (w, _) = arms.renderer.frame_size();
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
    // Sixteen letters in sixteen foreground colours on the default page: no
    // picture on a normal screen, and one under `?5`, where each cell's
    // drawn background is its foreground.
    feed(&mut t, b"\x1b[9;3H");
    for x in 0..16u32 {
        feed(&mut t, format!("\x1b[38;5;{}mX", 16 + x * 7).as_bytes());
    }
    feed(&mut t, b"\x1b[0m");
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
            usize::try_from(arms.renderer.frame_size().0.value()).unwrap(),
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
    // **Under `?5` the picture is found by the background drawn**, on both
    // arms: the 2x20 block still draws forty set colours as its backgrounds
    // (its foregrounds, exchanged), the sixteen letters become a picture,
    // and the fifteen-colour row and the panel draw the default foreground
    // as their backgrounds and join nothing. A cell arm whose overlay read
    // the stored background would find forty cells, not fifty-six.
    arms.renderer.set_theme(Theme {
        app_bg: baud_render::AppBg::Dimmed,
        picture_bg: baud_render::PictureBg::Opaque,
        ..resting.clone()
    });
    feed(&mut t, b"\x1b[?5h");
    let (a, b) = arms.draw(&mut t);
    assert_eq!(
        arms.exceptions.stats().pictured.value(),
        56,
        "?5: the cell arm's overlay found the picture's cells"
    );
    assert_eq!(
        differing(&a, &b),
        0,
        "?5: the arms disagree over a text-drawn picture"
    );
    let bg = arms.renderer.background();
    let alpha = |row: usize, col: usize| bg[row * cols.index() + col].0.to_be_bytes()[0];
    assert_eq!(alpha(1, 2), 0xFF, "?5: the block's first cell");
    assert_eq!(alpha(8, 2), 0xFF, "?5: the sixteen letters' first cell");
    assert_eq!(alpha(8, 17), 0xFF, "?5: the sixteen letters' last cell");
    assert_eq!(alpha(4, 2), 56, "?5: the fifteen blanks draw the page");
    feed(&mut t, b"\x1b[?5l");
    arms.renderer.set_theme(resting.clone());
    glass_floor_gate(&mut arms, &mut t, &resting);
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
    let iw = usize::try_from(arms.renderer.frame_size().0.value()).unwrap();
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
        arms.renderer
            .sync_styles(t, arms.styles_mem, &mut arms.stamp);
        arms.map.ensure(
            t,
            t.view(Rows::of_count(1)),
            arms.styles_mem,
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
    partial_image_gate(&mut arms, &mut t);
    cpu_page_gate(&mut arms, &mut t);
    kitty_draw_gate(&mut arms, &mut t);
    // **Styled faces.** Bold, italic and bold-italic text -- a ligature and
    // a Nerd Fonts icon among it -- on both arms, and a positive control
    // that each style reached the pixels: the same row regular paints
    // differently.
    {
        let (w, _) = arms.renderer.frame_size();
        let m = arms.renderer.metrics();
        let cell = (
            usize::try_from(m.cell_w.value()).unwrap(),
            usize::try_from(m.cell_h.value()).unwrap(),
        );
        let text = "ab -> x \u{f07b} ~= we\u{301}";
        let paint = |t: &mut Term<'_>, sgr: &str| {
            feed(t, b"\x1b[?25l\x1b[2J\x1b[H");
            for row in 0..3 {
                feed(
                    t,
                    format!("\x1b[{};1H\x1b[0;{sgr}m{text}", row + 2).as_bytes(),
                );
            }
        };
        paint(&mut t, "0");
        let (regular, _) = arms.draw(&mut t);
        for sgr in ["1", "3", "1;3"] {
            paint(&mut t, sgr);
            let (a, b) = arms.draw(&mut t);
            let bx = diff_box(&a, &b, usize::try_from(w.value()).unwrap(), cell);
            assert_eq!(
                differing(&a, &b),
                0,
                "the arms disagree under SGR {sgr}, in cells {bx:?}"
            );
            assert!(differing(&a, &regular) > 0, "SGR {sgr} reached the pixels");
        }
    }
    // **DEC line drawing.** The cell arm probes its map cell by cell; the
    // instance arm steps through runs by each character's width, which it
    // once read from `CellFlags::is_wide` -- the `GRAPHICS` bit on a grid
    // cell -- and drew every other line-drawing cell. The two arms must
    // agree, and the line must reach the pixels.
    {
        feed(&mut t, b"\x1b[0m\x1b[2J\x1b[H\x1b[?25l");
        let (blank, _) = arms.draw(&mut t);
        feed(&mut t, b"\x1b(0lqqqqqk\r\nx     x\r\nmqqqqqj\x1b(B");
        let (a, b) = arms.draw(&mut t);
        assert_eq!(
            differing(&a, &b),
            0,
            "the arms disagree on DEC line drawing"
        );
        assert!(
            differing(&a, &blank) > 0,
            "DEC line drawing reached the pixels"
        );
    }
    // **Text contrast: DirectWrite's grayscale curve.** A mask's coverage is
    // bent by the curve its colour's row names before it is blended, on both
    // arms through the one `glyph_fragment`. Two arms running one shader
    // agree about any curve, a missing one included, so the instance arm is
    // also held to a reference composited here from the atlas's own mask
    // through `baud_domain::contrast`: every glyph pixel within one step.
    // The control is the same reference with no curve, which must miss --
    // so a shader that skipped the lookup cannot pass.
    {
        let kept = (arms.highlight.take(), arms.window.take(), arms.trail.take());
        let (w, _) = arms.renderer.frame_size();
        let m = arms.renderer.metrics();
        let cell = (
            usize::try_from(m.cell_w.value()).unwrap(),
            usize::try_from(m.cell_h.value()).unwrap(),
        );
        let w = usize::try_from(w.value()).unwrap();
        let lines = [
            "\x1b[38;2;0;0;0;48;2;255;255;255m",
            "\x1b[38;2;255;255;255;48;2;0;0;0m",
            "\x1b[38;2;128;128;128;48;2;20;24;28m",
            "\x1b[38;2;30;90;200;48;2;250;240;220m",
        ];
        let text = "Hamburgefonstiv 0123 @#&";
        let paint = |t: &mut Term<'_>, body: &str| {
            let mut bytes = String::from("\x1b[0m\x1b[2J\x1b[H\x1b[?25l");
            for sgr in lines {
                bytes.push_str(sgr);
                bytes.push_str(body);
                bytes.push_str("\x1b[0m\r\n");
            }
            feed(t, bytes.as_bytes());
        };
        paint(&mut t, &" ".repeat(text.chars().count()));
        let (plain, _) = arms.draw(&mut t);
        paint(&mut t, text);
        let (a, b) = arms.draw(&mut t);
        let bx = diff_box(&a, &b, w, cell);
        assert_eq!(
            differing(&a, &b),
            0,
            "the arms disagree on the contrast curve, in cells {bx:?}"
        );
        let atlas = arms.renderer.atlas();
        let side = usize::try_from(atlas.side().value()).unwrap();
        let mask = atlas.pixels();
        let glyphs: Vec<Instance> = arms
            .renderer
            .instances()
            .filter(|i| i.kind == Kind::Mask)
            .copied()
            .collect();
        assert!(glyphs.len() > 60, "the scene drew its glyphs");
        let reference = |curved: bool| {
            let mut out = plain.clone();
            for g in &glyphs {
                let color = unit_color(g.color);
                for y in 0..usize::from(g.h.0) {
                    for x in 0..usize::from(g.w.0) {
                        let cov = mask[(usize::from(g.v.0) + y) * side + usize::from(g.u.0) + x];
                        let bent = if curved { g.curve.apply(cov) } else { cov };
                        let al = color[3] * f32::from(bent) / 255.0;
                        let src = [color[0] * al, color[1] * al, color[2] * al, al];
                        let px = usize::try_from(i32::from(g.x.0)).unwrap() + x;
                        let py = usize::try_from(i32::from(g.y.0)).unwrap() + y;
                        let at = py * w + px;
                        out[at] = over(src, out[at]);
                    }
                }
            }
            out
        };
        let off_by = |want: &[u32]| {
            a.iter()
                .zip(want)
                .filter(|&(&g, &r)| {
                    (0..4).any(|k| {
                        let (g, r) = ((g >> (8 * k)) & 0xff, (r >> (8 * k)) & 0xff);
                        g.abs_diff(r) > 1
                    })
                })
                .count()
        };
        let curved = off_by(&reference(true));
        assert_eq!(curved, 0, "the GPU's glyphs follow the contrast curve");
        assert!(
            off_by(&reference(false)) > 200,
            "the curve reaches the pixels: an uncurved reference must miss"
        );
        (arms.highlight, arms.window, arms.trail) = kept;
    }
    // **A glyph narrower than its two cells is centred in them (note Z,
    // decision B).** JetBrains Mono's `⚡ ﹢ ｛ ｝` are one cell's advance in a
    // two-cell span, `⌚` the emoji subset's. The cell arm learns the span
    // from the `WIDE_TAIL` in the next cell and keys its map on it; the
    // instance arm reads it with `CellSpan::at`. A mark on a wide base moves
    // with the base, and a block cursor and a selection swap over them.
    {
        let (w, _) = arms.renderer.frame_size();
        let m = arms.renderer.metrics();
        let cell = (
            usize::try_from(m.cell_w.value()).unwrap(),
            usize::try_from(m.cell_h.value()).unwrap(),
        );
        let w = usize::try_from(w.value()).unwrap();
        feed(&mut t, b"\x1b[0m\x1b[2J\x1b[H\x1b[2 q\x1b[?25l");
        feed(
            &mut t,
            "glyphs, quickly: \u{26a1}\u{fe62}\u{ff5b}\u{ff5d} \u{231a} \u{26a1}\u{301}\r\n\
             \x1b[38;2;0;205;0m\u{ff5b}x\u{ff5d}\x1b[0m\u{26a1}\r\nabcdefghijklmnopqrstuvwxyz\x1b[1;22H\x1b[?25h"
                .as_bytes(),
        );
        arms.highlight = Some(Highlight {
            from: (Row::from_index(1), Col::at_index(0)),
            to: (Row::from_index(1), Col::at_index(3)),
            region: Region::Stream,
        });
        let (a, b) = arms.draw(&mut t);
        let bx = diff_box(&a, &b, w, cell);
        assert_eq!(
            differing(&a, &b),
            0,
            "the arms disagree on a glyph centred in two cells, in cells {bx:?}"
        );
        arms.highlight = None;
        feed(&mut t, b"\x1b[2;1H\x1b[2K\x1b[1;18H\x1b[K");
        let (cleared, _) = arms.draw(&mut t);
        assert!(
            differing(&a, &cleared) > 0,
            "the centred glyphs reached the pixels"
        );
    }
    // **A variation selector picks the face (owner, 2026-09-30).** JetBrains
    // Mono carries (c) and (tm) as text and the emoji subset carries them in
    // colour: a bare one is the text glyph, `U+FE0F` asks for colour and
    // `U+FE0E` for text. The cell arm draws a bare one from its map and a
    // selected one from the overlay's records through `Renderer::cluster`;
    // the instance arm reads the selector per cell. Both must agree, and the
    // selector must reach the pixels.
    {
        feed(&mut t, b"\x1b[0m\x1b[2J\x1b[H\x1b[?25l");
        feed(
            &mut t,
            "selected: \u{a9} \u{a9}\u{fe0f} \u{2122}\u{fe0e} \u{2122}\u{fe0f}\r\n\
             abcdefghijklmnopqrstuvwxyz"
                .as_bytes(),
        );
        let (a, b) = arms.draw(&mut t);
        assert_eq!(
            differing(&a, &b),
            0,
            "the arms disagree on a variation selector"
        );
        feed(
            &mut t,
            "\x1b[1;11H\u{a9} \u{a9}  \u{2122}\u{fe0e} \u{2122} ".as_bytes(),
        );
        let (bare, _) = arms.draw(&mut t);
        assert!(differing(&a, &bare) > 0, "the selector reached the pixels");
    }
}

/// A one-band Sixel, 8 pixels wide and 6 high: shorter than any cell this
/// file draws, so the whole image lies inside one row.
const DOT: &[u8] = b"\x1bPq\"1;1;8;6#1;2;100;0;0#1!8~\x1b\\";

/// **A partly transparent image over coloured text** (V19 step 1). Both GPU
/// arms draw it through the same `draw_images`, so their agreeing says
/// nothing of alpha; what does is each pixel of its rect against the frame
/// drawn without it: the straight-alpha OVER, `(s a + d (255 - a)) / 255`
/// rounded, which `baud-render/tests/image.rs` holds the CPU compositor to
/// as well, so over an opaque page the three paths blend one way. A
/// snapshot that premultiplied before the fragment premultiplies again, or
/// a pass that drew a partial pixel whole, misses on most of them.
///
/// Over the glass page the CPU arm is held to the GPU arms by
/// `cpu_page_gate`.
fn partial_image_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    const W: usize = 40;
    const H: usize = 30;
    let pixel = |x: usize, y: usize| {
        let c = |v: usize| Chan::from_u8(u8::try_from(v % 256).unwrap());
        Pixel::straight(c(x * 7 + y * 13), c(x * 6), c(255 - y * 8), c(90 + x))
    };
    assert_eq!(
        arms.margin,
        (0, 0),
        "the frame is drawn at the target's origin"
    );
    feed(
        t,
        b"\x1bc\x1b[?25l\x1b[48;2;20;40;160m\x1b[38;2;250;220;30m\
          \x1b[2;1HHHHHHHHHHHHHHHHH\x1b[3;1HWWWWWWWWWWWWWWWW\x1b[4;1HMMMMMMMMMMMMMMMM\
          \x1b[5;1H################\x1b[0m\x1b[3;4H",
    );
    let (under_a, under_b) = arms.draw(t);
    let placed = t.put_image(
        ImagePx::of(u16::try_from(W).unwrap()),
        ImagePx::of(u16::try_from(H).unwrap()),
        ImageAlpha::Partial,
        |px| {
            for (i, p) in px.iter_mut().enumerate() {
                *p = pixel(i % W, i / W);
            }
        },
    );
    assert!(!placed.is_none(), "the partial image placed");
    let (a, b) = arms.draw(t);
    let places = arms.renderer.image_placements();
    assert_eq!(places.len(), 1, "the partial image is on screen");
    assert_eq!(places[0].alpha, ImageAlpha::Partial);
    let (x0, y0) = (
        usize::try_from(places[0].x).unwrap(),
        usize::try_from(places[0].y).unwrap(),
    );
    let fw = usize::try_from(arms.renderer.frame_size().0.value()).unwrap();
    let over = |src: Pixel, dst: u32| {
        let al = u32::from(src.alpha().get());
        let (r, g, bl) = src.rgb();
        let mix = |s: Chan, shift: u32| {
            (u32::from(s.get()) * al + ((dst >> shift) & 0xFF) * (255 - al) + 127) / 255
        };
        0xFF00_0000 | mix(r, 16) << 16 | mix(g, 8) << 8 | mix(bl, 0)
    };
    for (arm, under, got) in [("instance", &under_a, &a), ("cell", &under_b, &b)] {
        let (mut wrong, mut text) = (0, 0);
        for y in 0..H {
            for x in 0..W {
                let i = (y0 + y) * fw + x0 + x;
                assert_eq!(
                    under[i] >> 24,
                    0xFF,
                    "{arm}: the frame under the image is opaque"
                );
                if under[i] != under[(y0 + y) * fw + x0] {
                    text += 1;
                }
                if got[i] != over(pixel(x, y), under[i]) {
                    wrong += 1;
                }
            }
        }
        assert!(text > 0, "{arm}: the control, glyphs lie under the image");
        assert_eq!(
            wrong, 0,
            "{arm}: pixels not the OVER of the image on the frame under it"
        );
    }
    assert_eq!(
        differing(&a, &b),
        0,
        "the arms disagree with the partial image on screen"
    );
    feed(t, b"\x1bc");
}

/// **The floor on the page the glass draws**, on all three arms: dark
/// text on a light page the program set (gruvbox-light's pair, 10.2:1 as
/// asked, 1.14:1 dimmed over the dark page), drawn as a 2x20 picture of
/// forty pages, one cell of it a marked glyph the cell arm draws from a
/// record, as a one-page panel, under a selection and under a block
/// cursor. The panel's glyphs are lifted, so they differ from the same
/// frame with the floor off; the picture's, the selection's and the
/// cursor's are drawn solid and take the text asked, so they do not; nor
/// do box-drawing lines, on the panel and in herdr's divider colour on the
/// default page, while two block elements in that colour are floored. The
/// two GPU arms read one resolved table but choose between its two texts
/// in two spellings, `Resolved::drawn` and the shader's `colors()`, and
/// the CPU arm composites the instance arm's colours: this compares all
/// three, at the default dim.
fn glass_floor_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>, resting: &Theme) {
    assert_eq!(resting.bg_alpha.get(), 56, "the premise: the default dim");
    assert_eq!(arms.margin, (0, 0), "the premise: the frame at the origin");
    let pair = "38;2;60;56;54;48;2;251;241;199";
    feed(t, b"\x1b[?25l\x1b[2J\x1b[H\x1b[2 q");
    for y in 0..2u32 {
        feed(t, format!("\x1b[{};3H", y + 2).as_bytes());
        for x in 0..20u32 {
            // One marked glyph, which the cell arm draws from a record.
            let glyph = if (x, y) == (5, 0) { "e\u{301}" } else { "X" };
            feed(
                t,
                format!(
                    "\x1b[38;2;60;56;54;48;2;{};{};199m{glyph}",
                    251 - x,
                    241 - y * 8
                )
                .as_bytes(),
            );
        }
        feed(t, b"\x1b[0m");
    }
    feed(
        t,
        format!("\x1b[5;3H\x1b[{pair}mpanel text here\x1b[5;19H\u{2500}\x1b[0m").as_bytes(),
    );
    // herdr's divider colour on the default page, 1.05:1: four box-drawing
    // lines, one of them DEC Special Graphics' `q`, then two block elements.
    feed(
        t,
        "\x1b[7;3H\x1b[38;2;21;22;27m\u{2500} \u{2502} \u{256d} \u{257f} \x1b(0q\x1b(B \u{2580} \u{2588}\x1b[0m"
            .as_bytes(),
    );
    feed(t, b"\x1b[5;14H\x1b[?25h");
    arms.highlight = Some(Highlight {
        from: (Row::from_index(4), Col::at_index(2)),
        to: (Row::from_index(4), Col::at_index(6)),
        region: Region::Stream,
    });
    let fw = usize::try_from(arms.renderer.frame_size().0.value()).unwrap();
    let (cw, ch) = (
        usize::try_from(arms.renderer.metrics().cell_w.value()).unwrap(),
        usize::try_from(arms.renderer.metrics().cell_h.value()).unwrap(),
    );
    let mut frames = Vec::new();
    for floor in [baud_render::Floor::DEFAULT, baud_render::Floor::Off] {
        arms.renderer.set_theme(Theme {
            app_bg: baud_render::AppBg::Dimmed,
            picture_bg: baud_render::PictureBg::Opaque,
            floor,
            ..resting.clone()
        });
        let (a, b) = arms.draw(t);
        assert_eq!(
            arms.exceptions.stats().pictured.value(),
            40,
            "{floor:?}: the cell arm's overlay found the picture"
        );
        let bx = diff_box(&a, &b, fw, (cw, ch));
        assert_eq!(
            differing(&a, &b),
            0,
            "{floor:?}: the GPU arms disagree over the glass floor, cells {bx:?}"
        );
        let cpu = cpu_frame(arms, t);
        let all = channel_diff(&cpu, &a, |_| true);
        assert_eq!(
            all.0, 0,
            "{floor:?}: CPU and GPU differ on {} pixels, by up to {}",
            all.0, all.1
        );
        frames.push(b);
    }
    arms.highlight = None;
    feed(t, b"\x1b[?25l\x1b[0 q");
    // Pixels that differ between the floor on and off, over the cells of
    // `rows` by `cols`.
    let moved = |rows: std::ops::Range<usize>, cols: std::ops::Range<usize>| {
        let mut n = 0;
        for y in rows.start * ch..rows.end * ch {
            for x in cols.start * cw..cols.end * cw {
                n += usize::from(frames[0][y * fw + x] != frames[1][y * fw + x]);
            }
        }
        n
    };
    assert!(moved(4..5, 8..11) > 0, "the panel's glyphs were not lifted");
    assert_eq!(moved(4..5, 18..19), 0, "a line on the panel was lifted");
    assert_eq!(moved(6..7, 2..11), 0, "a box-drawing line was floored");
    for col in [12, 14] {
        assert!(
            moved(6..7, col..col + 1) > 0,
            "the block element at {col} was not floored"
        );
    }
    assert_eq!(moved(1..3, 2..22), 0, "a picture's text was lifted");
    assert_eq!(moved(4..5, 2..7), 0, "a selected cell's text was lifted");
    assert_eq!(moved(4..5, 13..14), 0, "the block cursor's cell was lifted");

    // **A bar cursor on a line keeps the floored colour it always had**: the
    // line rule is the text's, not the cursor's. All three arms draw it, and
    // the bar's pixels move with the floor.
    feed(t, b"\x1b[7;3H\x1b[6 q\x1b[?25h");
    let mut bars = Vec::new();
    for floor in [baud_render::Floor::DEFAULT, baud_render::Floor::Off] {
        arms.renderer.set_theme(Theme {
            app_bg: baud_render::AppBg::Dimmed,
            picture_bg: baud_render::PictureBg::Opaque,
            floor,
            ..resting.clone()
        });
        let (a, b) = arms.draw(t);
        assert_eq!(
            differing(&a, &b),
            0,
            "{floor:?}: the GPU arms disagree over a bar cursor on a line"
        );
        let cpu = cpu_frame(arms, t);
        let all = channel_diff(&cpu, &a, |_| true);
        assert_eq!(
            all.0, 0,
            "{floor:?}: CPU and GPU differ under a bar cursor on a line, by up to {}",
            all.1
        );
        bars.push(b[6 * ch * fw + 2 * cw]);
    }
    assert_ne!(
        bars[0], bars[1],
        "the bar cursor on a line was drawn as asked, not floored"
    );
    feed(t, b"\x1b[?25l\x1b[0 q");

    // **A selected line's background is the floored text, as a selected
    // letter's is** (the owner's call of 2026-10-17, decisions 702/703): a
    // swap hands the background `fg` and the line rule leaves it alone.
    // Row 8 holds `x` and `U+2502` in herdr's divider colour, which says
    // where each glyph inks; row 9 holds them again, selected. All three
    // arms draw it; on the pixels neither glyph inks, the selected line's
    // equal the selected letter's, and move with the floor.
    feed(
        t,
        "\x1b[9;41H\x1b[38;2;21;22;27mx\u{2502}\x1b[10;41Hx\u{2502}\x1b[0m".as_bytes(),
    );
    arms.highlight = Some(Highlight {
        from: (Row::from_index(9), Col::at_index(40)),
        to: (Row::from_index(9), Col::at_index(41)),
        region: Region::Stream,
    });
    let cell = |f: &[u32], row: usize, col: usize| -> Vec<u32> {
        (0..ch)
            .flat_map(|y| (0..cw).map(move |x| (row * ch + y) * fw + col * cw + x))
            .map(|i| f[i])
            .collect()
    };
    let mut seen = Vec::new();
    for floor in [baud_render::Floor::DEFAULT, baud_render::Floor::Off] {
        arms.renderer.set_theme(Theme {
            app_bg: baud_render::AppBg::Dimmed,
            picture_bg: baud_render::PictureBg::Opaque,
            floor,
            ..resting.clone()
        });
        let (a, b) = arms.draw(t);
        assert_eq!(
            differing(&a, &b),
            0,
            "{floor:?}: the GPU arms disagree over a selected line"
        );
        let cpu = cpu_frame(arms, t);
        let all = channel_diff(&cpu, &a, |_| true);
        assert_eq!(
            all.0, 0,
            "{floor:?}: CPU and GPU differ over a selected line, by up to {}",
            all.1
        );
        // The letter, the line and a blank beside them, unselected; then
        // the letter and the line, selected.
        seen.push([
            cell(&b, 8, 40),
            cell(&b, 8, 41),
            cell(&b, 8, 44),
            cell(&b, 9, 40),
            cell(&b, 9, 41),
        ]);
    }
    arms.highlight = None;
    let bare: Vec<usize> = (0..cw * ch)
        .filter(|&i| {
            seen.iter()
                .all(|f| f[0][i] == f[2][i] && f[1][i] == f[2][i])
        })
        .collect();
    assert!(
        bare.len() * 4 > cw * ch,
        "the control: most of the cell is bare, {} of {}",
        bare.len(),
        cw * ch
    );
    let pick = |floor: usize, k: usize| bare.iter().map(|&i| seen[floor][k][i]).collect::<Vec<_>>();
    assert_eq!(
        pick(0, 4),
        pick(0, 3),
        "a selected line's background is a selected letter's, floored"
    );
    assert_ne!(
        pick(0, 4),
        pick(1, 4),
        "the control: the floor moved the selected line's background"
    );
}

/// The CPU arm: [`Renderer::render_at`] into a [`Target`], each pixel then
/// put through the macOS presenter's own `premultiply`, as
/// `Presenter::present` copies a frame into its surface -- the form the
/// two GPU arms' readback is in.
fn cpu_frame(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) -> Vec<u32> {
    let (fw, fh) = arms.renderer.frame_size();
    let n = usize::try_from(fw.value() * fh.value()).unwrap();
    let mut px = vec![0u32; n];
    let mut target = Target::new(&mut px, fw, fw, fh).expect("target");
    arms.renderer
        .render_at(t, arms.back, &mut target, Damage::Full);
    px.iter().map(|&p| premultiply(Argb(p)).0).collect()
}

/// How many pixels of `a` and `b` differ, and by how much at most in one
/// channel, over the pixels `keep` admits.
fn channel_diff(a: &[u32], b: &[u32], keep: impl Fn(usize) -> bool) -> (usize, u32) {
    let (mut n, mut most) = (0, 0);
    for (i, (&x, &y)) in a.iter().zip(b).enumerate() {
        if x == y || !keep(i) {
            continue;
        }
        n += 1;
        for shift in [0, 8, 16, 24] {
            let (p, q) = ((x >> shift) & 0xFF, (y >> shift) & 0xFF);
            most = most.max(p.abs_diff(q));
        }
    }
    (n, most)
}

/// **The CPU arm draws what the GPU arms draw, over an opaque page and over
/// the default glass page** (`[glass] dim_dark = 0.22`, alpha 56): text in
/// the default and in set colours, a block cursor, and a 40x30 image of
/// partly transparent pixels over the text; then over the glass the same
/// image over text on a set background, dimmed as the app's default
/// `AppBg::Dimmed` dims it; and over each, three translucent cursors.
/// Source-over is W3C Compositing and Blending Level 1's 5.1,
/// `co = Cs as + Cb ab (1 - as)` premultiplied,
/// `ao = as + ab (1 - as)` (`[[source: w3c-compositing-1]]`), which the GPU
/// arms' `One, OneMinusSourceAlpha` blend is over premultiplied pixels; the
/// CPU arm keeps straight pixels and the presenter premultiplies them, so
/// its straight result must be the one that premultiplies to the GPU's.
///
/// Over the opaque page the control: every pixel equal. Over the glass page
/// a blend that mixes the page's colour as if it were opaque differs from
/// the GPU on 1156 of the image's 1200 pixels, by up to 47, and on 3590
/// pixels of text and page outside it; over the set background on 1189,
/// by up to 45.
///
/// Over each page too the cursor trail ([`trail_over_page`]) and text in a
/// foreground below opaque. The source alpha of either is a product of two
/// fractions, coverage and colour: taken by the CPU as a truncated or
/// quantised byte and by the GPU as a float, they differed by one on 5, 9
/// and 9 pixels of the quad, 145, 252 and 399 of the sweep, and 1820, 1855
/// and 159 of the text, opaque page first. Both now round it to a byte.
fn cpu_page_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    const W: usize = 40;
    const H: usize = 30;
    let pixel = |x: usize, y: usize| {
        let c = |v: usize| Chan::from_u8(u8::try_from(v % 256).unwrap());
        Pixel::straight(c(x * 7 + y * 13), c(x * 6), c(255 - y * 8), c(90 + x))
    };
    let glass = arms.renderer.theme().clone();
    assert_eq!(
        glass.bg_alpha.get(),
        56,
        "the premise: the default glass dim"
    );
    let fw = usize::try_from(arms.renderer.frame_size().0.value()).unwrap();
    // Text in the default colours, and `partial_image_gate`'s text on a
    // set background, which `AppBg::Dimmed`, the app's default, dims too.
    let plain: &[u8] = b"\x1bc\x1b[2 q\x1b[2;1HHHHHHHHHHHHHHHHH\x1b[3;1HWWWWWWWWWWWWWWWW\
        \x1b[4;1H\x1b[31mred \x1b[1;32mbold\x1b[0m MMMMMM\x1b[5;1H################\
        \x1b[7;1Hcursor here\x1b[3;4H";
    let set: &[u8] = b"\x1bc\x1b[2 q\x1b[48;2;20;40;160m\x1b[38;2;250;220;30m\
        \x1b[2;1HHHHHHHHHHHHHHHHH\x1b[3;1HWWWWWWWWWWWWWWWW\x1b[4;1HMMMMMMMMMMMMMMMM\
        \x1b[5;1H################\x1b[0m\x1b[7;1Hcursor here\x1b[3;4H";
    for (page, alpha, app_bg, text) in [
        ("opaque", 0xFF, glass.app_bg, plain),
        ("glass", 56, glass.app_bg, plain),
        (
            "glass, set backgrounds",
            56,
            baud_render::AppBg::Dimmed,
            set,
        ),
    ] {
        arms.renderer.set_theme(Theme {
            bg_alpha: Chan::from_u8(alpha),
            app_bg,
            ..glass.clone()
        });
        feed(t, text);
        let placed = t.put_image(
            ImagePx::of(u16::try_from(W).unwrap()),
            ImagePx::of(u16::try_from(H).unwrap()),
            ImageAlpha::Partial,
            |px| {
                for (i, p) in px.iter_mut().enumerate() {
                    *p = pixel(i % W, i / W);
                }
            },
        );
        assert!(!placed.is_none(), "{page}: the partial image placed");
        feed(t, b"\x1b[7;8H");
        let (a, b) = arms.draw(t);
        let places = arms.renderer.image_placements();
        assert_eq!(places.len(), 1, "{page}: the partial image is on screen");
        let (x0, y0) = (
            usize::try_from(places[0].x).unwrap(),
            usize::try_from(places[0].y).unwrap(),
        );
        let in_image = |i: usize| {
            let (x, y) = (i % fw, i / fw);
            (x0..x0 + W).contains(&x) && (y0..y0 + H).contains(&y)
        };
        let still = a.clone();
        let cpu = cpu_frame(arms, t);
        // The premise: the page under the image is the page asked for.
        let corner = a[a.len() - 1];
        assert_eq!(corner >> 24, u32::from(alpha), "{page}: the page's alpha");
        assert_eq!(differing(&a, &b), 0, "{page}: the GPU arms disagree");
        let image = channel_diff(&cpu, &a, in_image);
        let rest = channel_diff(&cpu, &a, |i| !in_image(i));
        eprintln!("cpu against instance, {page}: image {image:?}, rest {rest:?}");
        assert_eq!(
            image.0,
            0,
            "{page}: CPU and GPU differ on {} of the image's {} pixels, by up to {}",
            image.0,
            W * H,
            image.1
        );
        assert_eq!(
            rest.0, 0,
            "{page}: CPU and GPU differ on {} pixels outside the image, by up to {}",
            rest.0, rest.1
        );
        // A cursor drawn as a translucent solid over the page: a blinking
        // bar and underline half way through a fade, and the plasma's glow
        // (a steady style neither fades nor glows).
        let phase = |opacity: u8, glow: u8| CursorPhase {
            opacity: Chan::from_u8(opacity),
            glow: Chan::from_u8(glow),
            ..CursorPhase::HIDDEN
        };
        for (shape, p, what) in [
            (b"\x1b[5 q", phase(128, 0), "a bar half faded"),
            (b"\x1b[3 q", phase(128, 0), "an underline half faded"),
            (b"\x1b[1 q", phase(0, 200), "a block's glow"),
        ] {
            feed(t, shape);
            arms.renderer.set_cursor_blink(p);
            let (a, b) = arms.draw(t);
            assert_eq!(
                differing(&a, &b),
                0,
                "{page}, {what}: the GPU arms disagree"
            );
            assert!(
                differing(&a, &still) > 0,
                "{page}, {what}: the control, drawn"
            );
            let cpu = cpu_frame(arms, t);
            let all = channel_diff(&cpu, &a, |_| true);
            eprintln!("cpu against instance, {page}, {what}: {all:?}");
            assert_eq!(
                all.0, 0,
                "{page}, {what}: CPU and GPU differ on {} pixels, by up to {}",
                all.0, all.1
            );
        }
        arms.renderer.set_cursor_blink(Blink::Shown);
        trail_over_page(arms, t, page);
        let (opaque_fg, _) = arms.draw(t);
        // A glyph mask in a colour below opaque: the default foreground at
        // alpha 180, so a pixel's source alpha is the colour's times the
        // coverage, `as` of 5.1 a product of two fractions.
        arms.renderer.set_theme(Theme {
            bg_alpha: Chan::from_u8(alpha),
            app_bg,
            fg: glass.fg.with_alpha(Chan::from_u8(180)),
            ..glass.clone()
        });
        let (a, b) = arms.draw(t);
        assert_eq!(
            differing(&a, &b),
            0,
            "{page}, a sub-opaque mask: the GPU arms disagree"
        );
        assert!(
            differing(&a, &opaque_fg) > 0,
            "{page}, a sub-opaque mask: the control, drawn"
        );
        let cpu = cpu_frame(arms, t);
        let all = channel_diff(&cpu, &a, |_| true);
        eprintln!("cpu against instance, {page}, a sub-opaque mask: {all:?}");
        assert_eq!(
            all.0, 0,
            "{page}, a sub-opaque mask: CPU and GPU differ on {} pixels, by up to {}",
            all.0, all.1
        );
    }
    arms.renderer.set_theme(glass);
    feed(t, b"\x1bc");
}

/// [`cpu_page_gate`]'s cursor trail over the page as it stands: the quad
/// part way from a cell over the text to the cursor, sfte's sweep from the
/// same cell, and the fading ghost -- each drawn by both GPU arms and the
/// CPU arm, which paints the quad and the sweep by `trail::coverage` per
/// pixel (`Target::quad`) and the ghost as a translucent solid.
fn trail_over_page(arms: &mut Arms<'_, '_>, t: &mut Term<'_>, page: &str) {
    let (plain, _) = arms.draw(t);
    let (cr, cc) = t.cursor();
    let style = t.cursor_style();
    let hole = arms.renderer.cursor_rect(cr, cc, style);
    let from = arms
        .renderer
        .cursor_rect(Row::from_index(1), Col::at_index(14), style);
    let seg = Segment::new(from.corners(), hole);
    for (look, what) in [
        (
            TrailLook::Smear(Smear {
                corners: seg.at(0.3),
                hole,
                shape: Shape::Quad,
            }),
            "the trail's quad",
        ),
        (
            TrailLook::Smear(Smear {
                corners: from.corners(),
                hole,
                shape: Shape::Sweep,
            }),
            "the trail's sweep",
        ),
        (
            TrailLook::Ghost(Trail {
                from: (Row::from_index(1), Col::at_index(14)),
                to: (cr, cc),
                progress: 0.5,
                origin_dy: 0.0,
                lift: Px::of(0),
            }),
            "the trail's ghost",
        ),
    ] {
        arms.trail = Some(look);
        let (a, b) = arms.draw(t);
        assert_eq!(
            differing(&a, &b),
            0,
            "{page}, {what}: the GPU arms disagree"
        );
        assert!(
            differing(&a, &plain) > 0,
            "{page}, {what}: the control, drawn"
        );
        let cpu = cpu_frame(arms, t);
        let all = channel_diff(&cpu, &a, |_| true);
        eprintln!("cpu against instance, {page}, {what}: {all:?}");
        assert_eq!(
            all.0, 0,
            "{page}, {what}: CPU and GPU differ on {} pixels, by up to {}",
            all.0, all.1
        );
    }
    arms.trail = None;
}

/// The kitty gate's image, 6 by 4, every pixel opaque and distinct; and a
/// second, in other colours, for which of two is on top.
fn kitty_texel(x: u32, y: u32) -> u32 {
    0xFF00_0000 | ((37 * x + 20) << 16) | ((53 * y + 10) << 8) | 200
}

fn kitty_other(x: u32, y: u32) -> u32 {
    0xFF00_0000 | (200 << 16) | ((29 * x + 5) << 8) | (41 * y + 30)
}

/// `a=t` of a 6 by 4 `f=32` image `id` with `px`'s pixels, alpha the top
/// byte, then each
/// placement in `puts` at row 3, column 5 (one-based).
fn kitty_put(t: &mut Term<'_>, id: u32, px: fn(u32, u32) -> u32, puts: &[&str]) {
    const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut rgba = Vec::new();
    for y in 0..4 {
        for x in 0..6 {
            let [a, r, g, b] = px(x, y).to_be_bytes();
            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }
    let mut cmd = format!("\x1b_Ga=t,f=32,s=6,v=4,i={id},q=2;").into_bytes();
    for q in rgba.chunks(3) {
        let n = u32::from_be_bytes([0, q[0], q[1], q[2]]);
        for k in 0..4 {
            cmd.push(B64[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]);
        }
    }
    cmd.extend_from_slice(b"\x1b\\");
    feed(t, &cmd);
    for put in puts {
        feed(
            t,
            format!("\x1b[3;5H\x1b_Ga=p,i={id},q=2,{put}\x1b\\").as_bytes(),
        );
    }
}

/// Where the text says a placement of the 6 by 4 image draws, from the
/// anchor cell's top-left -- `[x, y, w, h]` -- and the source rectangle it
/// maps, the same four: `baud-render/tests/kitty_draw.rs`'s rules, written
/// again here so the GPU arms are held to the text and not to the
/// renderer's geometry.
fn kitty_expect(ctrl: &str, cw: u32, ch: u32) -> [[u32; 4]; 2] {
    let key = |name: char| {
        ctrl.split(',')
            .find_map(|kv| kv.strip_prefix(name)?.strip_prefix('='))
            .map_or(0u32, |v| v.parse().unwrap())
    };
    let (iw, ih) = (6u32, 4u32);
    let nearest = |a: u64, b: u64| u32::try_from((a + b / 2) / b).unwrap();
    let sx = key('x').min(iw);
    let sy = key('y').min(ih);
    let sw = if key('w') == 0 {
        iw - sx
    } else {
        key('w').min(iw - sx)
    };
    let sh = if key('h') == 0 {
        ih - sy
    } else {
        key('h').min(ih - sy)
    };
    let (sw64, sh64) = (u64::from(sw), u64::from(sh));
    let (ox, oy) = (key('X'), key('Y'));
    let (mut dx, mut dy) = (ox, oy);
    let (dw, dh) = match (key('c'), key('r')) {
        (0, 0) => (sw, sh),
        (c, 0) => {
            let dw = c * cw - ox;
            (dw, nearest(u64::from(dw) * sh64, sw64))
        }
        (0, r) => {
            let dh = r * ch - oy;
            (nearest(u64::from(dh) * sw64, sh64), dh)
        }
        (c, r) => {
            let (bw, bh) = (c * cw - ox, r * ch - oy);
            if u64::from(bw) * sh64 < u64::from(bh) * sw64 {
                let dh = nearest(u64::from(bw) * sh64, sw64);
                dy += (bh - dh) / 2;
                (bw, dh)
            } else {
                let dw = nearest(u64::from(bh) * sw64, sh64);
                dx += (bw - dw) / 2;
                (dw, bh)
            }
        }
    };
    [[dx, dy, dw, dh], [sx, sy, sw, sh]]
}

/// **Kitty placements draw what they store, on all three paths**
/// (`[[source: kitty-graphics]]`, "Controlling displayed image layout"):
/// the source rectangle, `c` and `r` with the aspect kept and a box
/// letterboxed, `X` and `Y`, and `z` -- negative under the text, and the
/// order by `z` then the lower id under. Each arm is held to the text's
/// pixels, computed here: the source texel under each drawn pixel's centre
/// inside the drawn rectangle, the frame without the image outside it, and
/// under the text the text's pixel where it drew one; then the arms to
/// each other and the CPU to them. Both GPU arms read the placements' tables
/// through one `image_fragment`, so their agreeing alone would say nothing.
fn kitty_draw_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    assert_eq!(arms.margin, (0, 0), "the frame is at the target's origin");
    let m = arms.renderer.metrics();
    let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
    let px16 = |v: u32| ImagePx::of(u16::try_from(v).unwrap());
    t.set_cell_size(px16(cw), px16(ch));
    let fw = arms.renderer.frame_size().0.value();
    let at = |x: u32, y: u32| usize::try_from(y * fw + x).unwrap();
    let (ax, ay) = (4 * cw, 2 * ch);
    let blocks = "\x1bc\x1b[?25l\x1b[3;5H\u{2580}\u{2580}\u{2580}\u{2580}\u{2580}\u{2580}";
    for (text, put) in [
        ("\x1bc\x1b[?25l", "x=2,y=1,w=3,h=2"),
        ("\x1bc\x1b[?25l", "c=3"),
        ("\x1bc\x1b[?25l", "r=2"),
        ("\x1bc\x1b[?25l", "c=4,r=1"),
        ("\x1bc\x1b[?25l", "c=1,r=3"),
        ("\x1bc\x1b[?25l", "X=3,Y=5,c=2"),
        ("\x1bc\x1b[?25l", "X=1,Y=2,x=1,y=1,w=4,h=2,c=5,r=1"),
        (blocks, "c=4,r=1,z=-1"),
        (blocks, "c=4,r=1,z=0"),
    ] {
        feed(t, text.as_bytes());
        let (before, _) = arms.draw(t);
        kitty_put(t, 1, kitty_texel, &[put]);
        let (a, b) = arms.draw(t);
        let cpu = cpu_frame(arms, t);
        let [[dx, dy, dw, dh], [sx, sy, sw, sh]] = kitty_expect(put, cw, ch);
        let under = put.contains("z=-1");
        let mut want = before.clone();
        for j in 0..dh {
            for i in 0..dw {
                let i_at = at(ax + dx + i, ay + dy + j);
                let texel = kitty_texel(
                    sx + (2 * i + 1) * sw / (2 * dw),
                    sy + (2 * j + 1) * sh / (2 * dh),
                );
                let bare = before[i_at] == before[at(0, 0)];
                if !under || bare {
                    want[i_at] = texel;
                }
            }
        }
        if under {
            let ink = (0..dh)
                .flat_map(|j| (0..dw).map(move |i| (i, j)))
                .filter(|&(i, j)| before[at(ax + dx + i, ay + dy + j)] != before[at(0, 0)])
                .count();
            assert!(ink > 0, "{put}: the premise, text under the image");
        }
        for (arm, got) in [("instance", &a), ("cell", &b), ("cpu", &cpu)] {
            assert_eq!(
                differing(got, &want),
                0,
                "{put}: the {arm} arm is not the text's pixels"
            );
        }
        assert_eq!(differing(&a, &b), 0, "{put}: the GPU arms disagree");
    }
    // The order: `z` first, then the lower id under, whatever the order of
    // placement -- the second placed is on top only when the rest tie.
    for (first, second, top) in [
        ((1, "z=2"), (2, "z=1"), kitty_texel(0, 0)),
        ((7, "z=0"), (3, "z=0"), kitty_texel(0, 0)),
        ((1, "z=1"), (2, "z=1"), kitty_other(0, 0)),
    ] {
        feed(t, b"\x1bc\x1b[?25l");
        kitty_put(t, first.0, kitty_texel, &[first.1]);
        kitty_put(t, second.0, kitty_other, &[second.1]);
        let (a, b) = arms.draw(t);
        let cpu = cpu_frame(arms, t);
        for (arm, got) in [("instance", &a), ("cell", &b), ("cpu", &cpu)] {
            assert_eq!(
                got[at(ax, ay)],
                top,
                "{first:?} then {second:?}: the {arm} arm's top image"
            );
        }
    }
    // A margin scroll clips: two rows at the top margin, scrolled up one,
    // draw their second row at the margin and nothing above it.
    feed(t, b"\x1bc\x1b[?25l\x1b[3;10r");
    let (before, _) = arms.draw(t);
    kitty_put(t, 1, kitty_texel, &["C=1,r=2"]);
    feed(t, b"\x1b[10;1H\n");
    let (a, b) = arms.draw(t);
    let cpu = cpu_frame(arms, t);
    let [[_, _, dw, dh], [sx, sy, sw, sh]] = kitty_expect("r=2", cw, ch);
    let mut want = before;
    for k in 0..ch {
        for i in 0..dw {
            want[at(ax + i, ay + k)] = kitty_texel(
                sx + (2 * i + 1) * sw / (2 * dw),
                sy + (2 * (ch + k) + 1) * sh / (2 * dh),
            );
        }
    }
    for (arm, got) in [("instance", &a), ("cell", &b), ("cpu", &cpu)] {
        assert_eq!(
            differing(got, &want),
            0,
            "a margin scroll: the {arm} arm does not clip at the margin"
        );
    }
    kitty_below_gate(arms, t);
    kitty_picks_gate(arms, t);
    kitty_placeholder_gate(arms, t);
    kitty_placeholder_leaving_gate(arms, t);
    t.set_cell_size(ImagePx::ZERO, ImagePx::ZERO);
    feed(t, b"\x1bc");
}

/// **Below `INT32_MIN/2`, under the non-default backgrounds, on all three
/// paths**: "Negative z-index values below INT32_MIN/2 (-1,073,741,824)
/// will be drawn under cells with non-default background colors"
/// (`[[source: kitty-graphics]]`). Four cells at the anchor -- a red-backed
/// upper half block, a red-backed space, a default-backed block and a
/// default-backed space -- under an image `c=4,r=1`: at -1073741825 the red
/// cells are the frame without the image and the default ones show it under
/// their ink; at -1073741824 exactly, which is not below, the image shows
/// in all four under the ink. Then the forks Jev decided, NON_DEFAULT: a
/// block cursor on the third cell and the selection on the fourth draw over
/// the image. Every expected pixel is the frame without the image or the
/// texel the text puts there, never another arm's.
fn kitty_below_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    let m = arms.renderer.metrics();
    let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
    let fw = arms.renderer.frame_size().0.value();
    let at = |x: u32, y: u32| usize::try_from(y * fw + x).unwrap();
    let (ax, ay) = (4 * cw, 2 * ch);
    let scene = "\x1bc\x1b[?25l\x1b[3;5H\x1b[41m\u{2580} \x1b[0m\u{2580} ";
    let lit = Highlight {
        from: (Row::from_index(2), Col::at_index(7)),
        to: (Row::from_index(2), Col::at_index(7)),
        region: Region::Stream,
    };
    for (z, deep, more, highlight) in [
        (-1_073_741_825, true, "", None),
        (-1_073_741_824, false, "", None),
        (
            -1_073_741_825,
            true,
            "\x1b[2 q\x1b[3;7H\x1b[?25h",
            Some(lit),
        ),
    ] {
        arms.highlight = highlight;
        feed(t, scene.as_bytes());
        feed(t, more.as_bytes());
        let (before, _) = arms.draw(t);
        let put = format!("C=1,c=4,r=1,z={z}");
        feed(t, b"\x1b7");
        kitty_put(t, 1, kitty_texel, &[&put]);
        feed(t, b"\x1b8");
        let (a, b) = arms.draw(t);
        let cpu = cpu_frame(arms, t);
        let [[dx, dy, dw, dh], [sx, sy, sw, sh]] = kitty_expect("c=4,r=1", cw, ch);
        let mut want = before.clone();
        let mut kinds = [0usize; 4];
        for j in 0..dh {
            for i in 0..dw {
                let (x, y) = (ax + dx + i, ay + dy + j);
                let cell = (dx + i) / cw;
                let ink = before[at(x, y)] != before[at(x, ay + ch - 1)];
                let lit_cell = highlight.is_some() && cell >= 2;
                let hidden = ink || lit_cell || (deep && cell < 2);
                kinds[2 * usize::from(cell >= 2) + usize::from(ink)] += 1;
                if !hidden {
                    want[at(x, y)] = kitty_texel(
                        sx + (2 * i + 1) * sw / (2 * dw),
                        sy + (2 * j + 1) * sh / (2 * dh),
                    );
                }
            }
        }
        assert!(kinds.iter().all(|&k| k > 0), "z={z}: the premise {kinds:?}");
        for (arm, got) in [("instance", &a), ("cell", &b), ("cpu", &cpu)] {
            assert_eq!(
                differing(got, &want),
                0,
                "z={z}, {more:?}: the {arm} arm is not the text's pixels"
            );
        }
        assert_eq!(differing(&a, &b), 0, "z={z}: the GPU arms disagree");
    }
    arms.highlight = None;
    feed(t, b"\x1b[0 q");
}

/// What a pixel of a bare cell under a deep image is expected to be: the
/// image's texel, the frame without the image, or the cell's background,
/// straight, source-over the texel.
#[derive(Clone, Copy, Debug)]
enum Deep {
    Texel,
    Plain,
    Over(Argb),
}

/// `bg`, straight with its alpha, source-over the opaque texel `t`, one
/// channel at a time and rounded to the nearest byte: `co = Cs as + Cb (1 -
/// as)` at `ab = 1` (`[[source: w3c-compositing-1]]` 5.1).
fn over_texel(bg: Argb, t: u32) -> u32 {
    let (w, a) = (bg.0, bg.0 >> 24);
    let ch = |shift: u32| {
        let (s, d) = ((w >> shift) & 0xFF, (t >> shift) & 0xFF);
        ((s * a + d * (255 - a) + 127) / 255) << shift
    };
    0xFF00_0000 | ch(16) | ch(8) | ch(0)
}

/// One scene of [`kitty_picks_gate`]: the text written at the anchor, the
/// theme, the image's pixels and what each of the four cells shows.
type PicksScene<'t> = (String, &'t Theme, fn(u32, u32) -> u32, [Deep; 4]);

/// The kitty gate's image with every pixel clear.
fn kitty_clear(x: u32, y: u32) -> u32 {
    kitty_texel(x, y) & 0x00FF_FFFF
}

/// **The owner's two picks for an image below `INT32_MIN/2`, on all three
/// paths** (2026-10-16). "Compare the colour": a cell is default when the
/// colour its background is drawn in is the default background's, so an
/// explicit 24-bit colour or an `OSC 4` entry of the default's value shows
/// the image, and after `OSC 11` a cell of the new default's value does
/// and one of the old default's does not. "Picture shows dimmed": on the
/// glass a coloured cell dimmed as `AppBg::Dimmed` dims it is its
/// background source-over the image, and over a clear image the frame
/// without it; an opaque one, the gate's own `AppBg::Opaque`, still
/// replaces. Four bare cells at the anchor under an image `c=4,r=1`; every
/// expected pixel the frame without the image, the texel, or the
/// background over the texel by the formula, never another arm's.
fn kitty_picks_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    let m = arms.renderer.metrics();
    let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
    let fw = arms.renderer.frame_size().0.value();
    let at = |x: u32, y: u32| usize::try_from(y * fw + x).unwrap();
    let (ax, ay) = (4 * cw, 2 * ch);
    let glass = arms.renderer.theme().clone();
    assert_eq!(
        (glass.bg.0 & 0xFF_FFFF, glass.bg_alpha.get(), glass.app_bg),
        (0x10_1010, 56, baud_render::AppBg::Opaque),
        "the premise: the gate's glass"
    );
    let dimmed = Theme {
        app_bg: baud_render::AppBg::Dimmed,
        ..glass.clone()
    };
    let red = glass.palette[1].with_alpha(glass.bg_alpha);
    let (texel, plain, over) = (Deep::Texel, Deep::Plain, Deep::Over(red));
    let osc = "\x1b]11;rgb:30/00/30\x1b\\";
    let scenes: [PicksScene<'_>; 7] = [
        (
            "\x1b[48;2;16;16;16m  \x1b[0m  ".into(),
            &glass,
            kitty_texel,
            [texel; 4],
        ),
        (
            "\x1b]4;1;rgb:10/10/10\x1b\\\x1b[41m  \x1b[0m  ".into(),
            &glass,
            kitty_texel,
            [texel; 4],
        ),
        (
            format!("{osc}\x1b[48;2;48;0;48m  \x1b[0m  "),
            &glass,
            kitty_texel,
            [texel; 4],
        ),
        (
            format!("{osc}\x1b[48;2;16;16;16m  \x1b[0m  "),
            &glass,
            kitty_texel,
            [plain, plain, texel, texel],
        ),
        (
            "\x1b[41m  \x1b[0m  ".into(),
            &dimmed,
            kitty_texel,
            [over, over, texel, texel],
        ),
        (
            "\x1b[41m  \x1b[0m  ".into(),
            &dimmed,
            kitty_clear,
            [plain; 4],
        ),
        (
            "\x1b[41m  \x1b[0m  ".into(),
            &glass,
            kitty_texel,
            [plain, plain, texel, texel],
        ),
    ];
    for (text, theme, px, want_of) in scenes {
        arms.renderer.set_theme(theme.clone());
        feed(t, format!("\x1bc\x1b[?25l\x1b[3;5H{text}").as_bytes());
        let (before, _) = arms.draw(t);
        kitty_put(t, 1, px, &["C=1,c=4,r=1,z=-1073741825"]);
        let (a, b) = arms.draw(t);
        let cpu = cpu_frame(arms, t);
        let [[dx, dy, dw, dh], [sx, sy, sw, sh]] = kitty_expect("c=4,r=1", cw, ch);
        let mut want = before.clone();
        for j in 0..dh {
            for i in 0..dw {
                let (x, y) = (ax + dx + i, ay + dy + j);
                let cell = usize::try_from((dx + i) / cw).unwrap();
                let tx = kitty_texel(
                    sx + (2 * i + 1) * sw / (2 * dw),
                    sy + (2 * j + 1) * sh / (2 * dh),
                );
                want[at(x, y)] = match want_of[cell] {
                    Deep::Texel => tx,
                    Deep::Plain => before[at(x, y)],
                    Deep::Over(bg) => over_texel(bg, tx),
                };
            }
        }
        for (arm, got) in [("instance", &a), ("cell", &b), ("cpu", &cpu)] {
            let (n, most) = channel_diff(got, &want, |_| true);
            assert_eq!(
                n, 0,
                "{text:?}, {want_of:?}: the {arm} arm misses {n} pixels, by up to {most}"
            );
        }
    }
    // A half-transparent image under the dimmed cells, both kinds: the
    // arms agree, where the CPU's clear and source-over each round once.
    arms.renderer.set_theme(dimmed);
    feed(t, b"\x1bc\x1b[?25l\x1b[3;5H\x1b[41m  \x1b[0m  ");
    let (before, _) = arms.draw(t);
    kitty_put(t, 1, kitty_half, &["C=1,c=4,r=1,z=-1073741825"]);
    let (a, b) = arms.draw(t);
    let cpu = cpu_frame(arms, t);
    assert!(
        differing(&a, &before) > 0,
        "a half image: the premise, drawn"
    );
    for (arm, got) in [("cell", &b), ("cpu", &cpu)] {
        let (n, most) = channel_diff(got, &a, |_| true);
        assert_eq!(
            n, 0,
            "a half image: the {arm} arm and the instance arm differ on {n} pixels, by up to {most}"
        );
    }
    arms.renderer.set_theme(glass);
    feed(t, b"\x1bc");
}

/// The kitty gate's image at half alpha.
fn kitty_half(x: u32, y: u32) -> u32 {
    kitty_texel(x, y) & 0x00FF_FFFF | 0x8000_0000
}

/// The pinned list's diacritic for `n` (`[[source:
/// kitty-rowcolumn-diacritics]]`), read from the list and not the table
/// generated from it.
fn placeholder_diacritic(n: usize) -> char {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/kitty-rowcolumn-diacritics.txt"
    );
    let src = std::fs::read_to_string(path).unwrap();
    let line = src
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .nth(n)
        .unwrap();
    char::from_u32(u32::from_str_radix(line.split(';').next().unwrap(), 16).unwrap()).unwrap()
}

/// U+10EEEE cells at `cells` -- `(row, col)` from the gate's anchor, row 3
/// and column 5 one-based -- in `sgr`: every cell its row and column and,
/// past zero, the id's top byte `msb` when `full`, else only each row's
/// first cell its row, the rest inheriting.
fn placeholder_grid(sgr: &str, cells: &[(usize, usize)], msb: usize, full: bool) -> Vec<u8> {
    let d = placeholder_diacritic;
    let mut s = String::new();
    let mut rows: Vec<usize> = cells.iter().map(|c| c.0).collect();
    rows.dedup();
    for r in rows {
        let cols: Vec<usize> = cells.iter().filter(|c| c.0 == r).map(|c| c.1).collect();
        s += &format!("\x1b[{};{}H\x1b[0;{sgr}m", 3 + r, 5 + cols[0]);
        for (k, c) in cols.into_iter().enumerate() {
            s.push('\u{10EEEE}');
            match (full, k) {
                (true, _) => {
                    s.push(d(r));
                    s.push(d(c));
                    if msb != 0 {
                        s.push(d(msb));
                    }
                }
                (false, 0) => s.push(d(r)),
                _ => {}
            }
        }
    }
    s += "\x1b[0m";
    s.into_bytes()
}

/// **kitty's Unicode placeholders on every arm** (`[[source:
/// kitty-graphics]]`, "Unicode placeholders"): a grid of U+10EEEE cells
/// at the anchor draws, on the instance, cell and CPU arms alike, what a
/// placement of its virtual placement's box draws on that arm -- which the
/// gate above holds to the text -- and a partial grid draws that inside
/// its cells and nothing outside them. The scenes reach the inheritance,
/// the id's top byte and the placement id in the underline colour, each
/// of which draws other pixels when it is ignored; the image is 6 by 4 and
/// the boxes are not square, so a row and a column swapped do too.
fn kitty_placeholder_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    let m = arms.renderer.metrics();
    let (cw, ch) = (
        usize::try_from(m.cell_w.value()).unwrap(),
        usize::try_from(m.cell_h.value()).unwrap(),
    );
    let fw = usize::try_from(arms.renderer.frame_size().0.value()).unwrap();
    let frames = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>| {
        let (a, b) = arms.draw(t);
        let cpu = cpu_frame(arms, t);
        [a, b, cpu]
    };
    let all = |c: usize, r: usize| -> Vec<(usize, usize)> {
        (0..r).flat_map(|y| (0..c).map(move |x| (y, x))).collect()
    };
    let hi = 1 + (2 << 24);
    feed(t, b"\x1bc\x1b[?25l");
    let blank = frames(arms, t);
    // (name, the image placed, its virtual placements, the grid).
    type Scene<'s> = (&'s str, (u32, fn(u32, u32) -> u32), &'s [&'s str], Vec<u8>);
    let scenes: [Scene<'_>; 5] = [
        ("4x3, every cell", (1, kitty_texel), &["U=1,c=4,r=3"], {
            placeholder_grid("38;5;1", &all(4, 3), 0, true)
        }),
        ("2x3, rows inherited", (1, kitty_texel), &["U=1,c=2,r=3"], {
            placeholder_grid("38;5;1", &all(2, 3), 0, false)
        }),
        ("5x1, the top byte", (hi, kitty_other), &["U=1,c=5,r=1"], {
            placeholder_grid("38;5;1", &all(5, 1), 2, true)
        }),
        (
            "2x3, placement 2",
            (1, kitty_texel),
            &["U=1,p=1,c=4,r=3", "U=1,p=2,c=2,r=3"],
            { placeholder_grid("38;5;1;58;5;2", &all(2, 3), 0, true) },
        ),
        ("4x3, three cells", (1, kitty_texel), &["U=1,c=4,r=3"], {
            placeholder_grid("38;5;1", &[(1, 1), (1, 2), (2, 3)], 0, true)
        }),
    ];
    for (name, (id, px), virts, grid) in scenes {
        let last = virts[virts.len() - 1];
        let geometry = last.trim_start_matches("U=1,").trim_start_matches("p=2,");
        feed(t, b"\x1bc\x1b[?25l");
        kitty_put(t, id, px, &[geometry]);
        let placed = frames(arms, t);
        assert!(
            differing(&placed[0], &blank[0]) > 0,
            "{name}: the premise, the placement draws"
        );
        feed(t, b"\x1bc\x1b[?25l");
        if id == hi {
            kitty_put(t, 1, kitty_texel, &["U=1,c=5,r=1"]);
        }
        kitty_put(t, id, px, virts);
        feed(t, &grid);
        let got = frames(arms, t);
        // The cells the grid wrote, on screen.
        let written: Vec<(usize, usize)> = match name {
            "4x3, three cells" => vec![(3, 5), (3, 6), (4, 7)],
            _ => Vec::new(),
        };
        for (k, arm) in ["instance", "cell", "cpu"].into_iter().enumerate() {
            let want: Vec<u32> = (0..got[k].len())
                .map(|i| {
                    let cell = (i / fw / ch, i % fw / cw);
                    match written.is_empty() || written.contains(&cell) {
                        true => placed[k][i],
                        false => blank[k][i],
                    }
                })
                .collect();
            assert_eq!(
                differing(&got[k], &want),
                0,
                "{name}: the {arm} arm's placeholders are not the placement's pixels"
            );
        }
    }
}

/// **A placeholder picture in a smooth scroll's leaving row** (`[[source:
/// kitty-graphics]]`, "Unicode placeholders"): an 11 by 3 picture whose
/// first row -- two cells, the second inheriting its row and column from
/// the first -- is the top row, scrolled one line. Its first row is then
/// the leaving row, the line above the window. Sliding, on the cell arm
/// over the live ring and over the window's copy, at a whole row and at
/// half of one, every pixel is the frame before the scroll moved down by
/// what is left of the slide: the leaving row's strip is the one that row
/// showed on screen. Settled, on all four arms, every pixel is the frame
/// before the scroll moved up a row: the leaving row's strip, placed at
/// a negative y, is clipped. At `z` 0, over the text; -1, under it; and
/// below `INT32_MIN/2`, which a placeholder clamps to under its cells'
/// backgrounds. The instance and CPU arms do not slide (`stage_images`,
/// "The instance path does not slide"), so they are held settled only.
fn kitty_placeholder_leaving_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    let m = arms.renderer.metrics();
    let ch = usize::try_from(m.cell_h.value()).unwrap();
    let fw = usize::try_from(arms.renderer.frame_size().0.value()).unwrap();
    let rows = t.view(Rows::ZERO).rows().index();
    let row = ch * fw;
    for z in ["", ",z=-1", ",z=-1073741825"] {
        arms.back = Rows::ZERO;
        arms.slide = None;
        feed(t, b"\x1bc\x1b[?25l");
        let (_, blank) = arms.draw(t);
        kitty_put(t, 1, kitty_texel, &[&format!("U=1,c=11,r=3{z}")]);
        let mut grid = placeholder_grid("38;5;1", &[(0, 0), (0, 1)], 0, false);
        grid.extend(placeholder_grid("38;5;1", &[(1, 2), (2, 2)], 0, true));
        feed(t, &grid);
        // The picture's first row, written on screen row 3, to the top.
        let line = t.image_origin(Rows::ZERO).top.value() + 2;
        feed(t, format!("\x1b[{rows};1H").as_bytes());
        while t.image_origin(Rows::ZERO).top.value() < line {
            feed(t, b"\n");
        }
        let (a, b) = arms.draw(t);
        let c = arms.draw_copied(t);
        let cpu = cpu_frame(arms, t);
        let before = [a, b, c, cpu];
        assert!(
            differing(&before[1][..row], &blank[..row]) > 0,
            "{z}: the premise, the picture's first row on the top row"
        );
        feed(t, b"\n");
        assert_eq!(
            t.image_origin(Rows::ZERO).top.value(),
            line + 1,
            "{z}: the picture's first row is the leaving row"
        );
        // Settled: every arm the frame before, a row up.
        let (a, b) = arms.draw(t);
        assert_eq!(
            arms.renderer.image_placements().len(),
            3,
            "{z}: the premise, the leaving row's strip and the two below"
        );
        let c = arms.draw_copied(t);
        let cpu = cpu_frame(arms, t);
        let n = (rows - 1) * row;
        for (k, (arm, got)) in [("instance", a), ("cell", b), ("copied", c), ("cpu", cpu)]
            .into_iter()
            .enumerate()
        {
            let d = differing(&got[..n], &before[k][row..row + n]);
            assert_eq!(
                d, 0,
                "{z}: settled, the {arm} arm shows the leaving row, {d} pixels"
            );
        }
        // Sliding: the cell arms the frame before, down by the pan left.
        arms.renderer
            .sync_styles(t, arms.styles_mem, &mut arms.stamp);
        arms.map.ensure(
            t,
            t.view(Rows::of_count(1)),
            arms.styles_mem,
            &mut arms.renderer,
            &mut arms.candidates,
        );
        for pan in [ch, ch / 2] {
            arms.slide = Some(Px::of(u32::try_from(pan).unwrap()));
            let (_, slid) = arms.draw(t);
            let copied = arms.draw_copied(t);
            arms.slide = None;
            let off = (ch - pan) * fw;
            let n = rows * row - off;
            for (k, arm, got) in [(1, "cell", slid), (2, "copied", copied)] {
                let d = differing(&got[..n], &before[k][off..off + n]);
                assert_eq!(
                    d, 0,
                    "{z}: a slide of {pan} on the {arm} arm is not the frame before, {d} pixels"
                );
            }
        }
    }
    feed(t, b"\x1bc");
}
