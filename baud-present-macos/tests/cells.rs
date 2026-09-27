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
    BytePos, Chan, Col, Cols, Depth, ImageBudget, PackedCell, ReadLen, Row, RowRot, Rows, WinSize,
};
use baud_font::{Face, Px};
use baud_present_macos::{
    CellInput, Done, Drawn, Gpu, GridBuffer, MapBuffer, OverlayBuffer, StylesBuffer,
};
use baud_render::effect::{Effect, Ignition, Seed, Vfx};
use baud_render::fire;
use baud_render::ice;
use baud_render::lightning;
use baud_render::plasma;
use baud_render::{
    Argb, Blend, Blink, Candidate, Corners, Damage, GlyphMap, Highlight, Hover, Instance, Kind,
    Look, Overlay, Pt, PxSize, Rect, Region, Renderer, Resolved, Scene, Scroll, Scroller, Segment,
    Shape, Smear, Stack, StyleStamp, Theme, Trail, TrailLook, Transition, VideoFlash, Widths,
    Window, quad_coverage,
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
/// and 15 -- the four `?5` swaps -- so the reverse-video scene draws the
/// palette swap through both arms and not only the defaults. Both read it
/// from one resolved table, so this compares their drawing of it; the
/// swap's own rule is gated in `baud-render/tests/reverse_video.rs`.
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
    /// Where the window sits and how the thumb is seen, for the scrollbar
    /// both arms draw.
    window: Option<Scroll>,
    /// The cursor trail both arms draw, or none.
    trail: Option<TrailLook>,
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
        // Both arms draw the same image placements: computed once here, read by
        // `draw` and `draw_cells` alike.
        self.renderer.set_images(t, self.back);
        self.map
            .ensure(view, &mut self.renderer, &mut self.candidates);
        // The app's rule: the window by the core's clamp, so the alternate
        // screen -- which keeps no history -- has its cursor whatever
        // window the scene carries.
        let cursor = (t.cursor_visible() && t.looking_back(self.back).index() == 0)
            .then(|| {
                let (r, c) = t.cursor();
                (r, c, t.cursor_style())
            })
            .filter(|&(_, _, style)| self.renderer.cursor_drawn(style));
        self.renderer
            .sync_styles(t, self.styles_mem, &mut self.stamp);
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
        // The app's rule: the window by the core's clamp, so the alternate
        // screen -- which keeps no history -- has its cursor whatever
        // window the scene carries.
        let cursor = (t.cursor_visible() && t.looking_back(self.back).index() == 0)
            .then(|| {
                let (r, c) = t.cursor();
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
    let s = Pt {
        x: p.x / c * 2.6 - f.dir.x * (f.age * fire::DRIFT),
        y: (p.y / c + f.age * fire::RISE) * 0.7 - f.dir.y * (f.age * fire::DRIFT),
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
) -> Vec<u32> {
    effect_frame_by(reference(vfx.effect), vfx, records, plain, width, origin)
}

/// The same, by any rule: the plasma's sheath alone is one.
fn effect_frame_by(
    rule: Reference,
    vfx: &Vfx,
    records: &[Instance],
    plain: &[u32],
    width: Px,
    origin: (usize, usize),
) -> Vec<u32> {
    let tw = usize::try_from(width.value()).unwrap();
    // The frame the records are clipped to, as the GPU's viewport clips
    // them: the target less its margin on each side. A spark thrown past the
    // frame's edge would otherwise land in the margin, or wrap onto the next
    // row of the target.
    let fw = i32::try_from(tw - 2 * origin.0).unwrap();
    let fh = i32::try_from(plain.len() / tw - 2 * origin.1).unwrap();
    let mut out = plain.to_vec();
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
                out[at] = over(src, out[at]);
            }
        }
    }
    out
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
        // and its width table is never read.
        let across = (cell_at(98.0, 2.0), cell_at(116.0, 3.0));
        let down = (cell_at(104.0, 0.0), cell_at(106.0, 8.0));
        let line = (bar(cell_at(98.0, 4.0)), bar(cell_at(116.0, 4.0)));
        for (name, (from, to), (quad_from, hole), scale) in [
            ("across", across, across, 1.0f32),
            ("down", down, down, 1.0),
            ("underline", line, line, 1.0),
            ("retarget", across, (across.1, cell_at(104.0, 8.0)), 1.0),
            ("large", across, across, 2.5),
        ] {
            // The fire's four ages, shrunk for an effect that is out sooner:
            // the lightning's, out at 0.32 s, run from its leader mid-path to
            // its last fade. A longer life keeps the fire's.
            let life = (effect.life() / Effect::Fire.life()).min(1.0);
            let ages = [0.04f32, 0.12, 0.25, 0.4].map(|a| a * life);
            let mut particles = false;
            let seg = Segment::new(quad_from.corners(), hole);
            let axis = {
                let (a, b) = (centre(quad_from), centre(hole));
                let l = (b.x - a.x).hypot(b.y - a.y);
                Pt {
                    x: (b.x - a.x) / l,
                    y: (b.y - a.y) / l,
                }
            };
            for age in ages {
                let vfx = Vfx {
                    effect,
                    emitter: Smear {
                        corners: seg.at(age),
                        hole,
                        shape: Shape::Quad,
                    },
                    ignition: Ignition::ignite(centre(from), to, Seed(7), ch * scale).at(age),
                    axis,
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
                let want = effect_frame(&vfx, records, &plain, width, origin);
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
                    off += usize::from(d == 1);
                    if x != plain[i] {
                        lit += 1;
                        assert!(inside(i), "{at}: pixel {i} changed outside every record");
                    }
                    assert!(
                        (0..3).all(|k| ch(x, k) <= ch(x, 3)),
                        "{at}: pixel {i} {x:08x} stores colour past its alpha"
                    );
                }
                assert!(off <= 20, "{at}: {off} pixels a step off the reference");
                if effect == Effect::Plasma {
                    // Nothing but the sheath ahead of the cursor: the
                    // filaments lie on the axis there, and ungated they
                    // poked a spike out of its leading edge.
                    let sheath = effect_frame_by(
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
                    assert!(ahead > 100, "{at}: only {ahead} pixels ahead of the cursor");
                }
                // A retarget's fire burns on the path the quad left, and the
                // plasma starves as (1 - k)^2: at the last age both are
                // mostly out.
                let least = if name == "retarget" || age >= ages[3] {
                    150
                } else {
                    300
                };
                assert!(lit > least, "{at}: only {lit} pixels lit");
                particles |= records.iter().any(|r| r.kind == Kind::Particle);
            }
            assert!(particles, "{effect:?} {name}: no particle at any age");
        }
    }
    arms.trail = None;
    arms.margin = (0, 0);
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
    }
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
    arms.renderer.set_blink(Blink::Hidden);
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
    arms.renderer.set_blink(Blink::Shown);
    let (on_a, _) = arms.draw(&mut t);
    assert_ne!(
        differing(&on_a, &off_a),
        0,
        "blinking text's off phase reached no pixel"
    );
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
    // The cursor trail: a fading ghost mid-sweep between two cells, drawn
    // by both arms -- one translucent solid on the instance list and on the
    // cell path's records -- and a change to the pixels. `plain` has the
    // same cursor and no trail, so the difference is the ghost alone.
    arms.trail = Some(TrailLook::Ghost(Trail {
        from: (Row::from_index(1), Col::at_index(2)),
        to: (Row::from_index(4), Col::at_index(20)),
        progress: 0.5,
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
    // `?5`, reverse video: every rendition resolves under the swap, the
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
    arms.renderer.set_flash(VideoFlash::Off);
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
