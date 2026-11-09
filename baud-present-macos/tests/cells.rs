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

use baud_core::{Owned, Term, WrapPending};
use baud_domain::CacheGeometry;
use baud_domain::themefile::CellPaint;
use baud_domain::{
    Animated, BytePos, Chan, Codepoint, Col, Cols, Depth, HostTime, ImageAlpha, ImageBudget,
    ImageMotion, ImagePx, PackedCell, Pixel, ReadLen, Row, RowRot, Rows, WinSize,
};
use baud_font::{Face, Px};
use baud_present_macos::journal::SetIx;
use baud_present_macos::redraw::{Ages, SURFACES, SurfaceIx};
use baud_present_macos::screens::{Compare, Screens};
use baud_present_macos::{
    CellInput, Done, Drawn, Gpu, GridBuffer, MapBuffer, OverlayBuffer, StylesBuffer, premultiply,
};
use baud_render::effect::{Effect, Ignition, Seed, Vfx};
use core::time::Duration;

use baud_render::fire;
use baud_render::ice;
use baud_render::lightning;
use baud_render::liquid;
use baud_render::orb;
use baud_render::plasma;
use baud_render::ufo;
use baud_render::{
    Argb, BellFlash, BellKind, BellShown, Blend, Blink, BlinkPhases, Candidate, Caret, Composition,
    Corners, CursorLook, CursorPhase, Damage, FaceRole, FaceStyle, FontSlot, GlyphMap, Highlight,
    Hover, Instance, Kind, Look, Mix, Overlay, Pt, PxSize, Rang, Rect, Region, Renderer, Resolved,
    Scene, ScreenSlide, Scroll, Scroller, Segment, Shape, SixelBg, Smear, SplashLook, SplashPhase,
    Stack, StatusMarks, StyleStamp, Tally, Target, Theme, Toward, Trail, TrailLook, Transition,
    VideoFlash, Widths, Window, compose_slide, quad_coverage, slide_offsets,
};

#[path = "../../baud-font/tests/sha256/mod.rs"]
mod sha256;

// What mpv sent, parsed from its capture with nothing of baud's: the
// parser `baud-app`'s replay of the capture reads too, which also reads
// the status lines this file does not look at.
#[allow(dead_code)]
#[path = "../../baud-app/src/film/mpv/capture.rs"]
mod mpv_capture;

/// **Wide enough for the right-hand side to be a question.** It was 40x6,
/// and a scene that never reaches a far column cannot show a path that
/// stops short of one -- which is what asciiquarium was reported doing,
/// with its waterline ending two thirds across a window whose cells the
/// core had filled to the last column.
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
/// does not: no ligature pair, no combining mark. The second box row is the
/// procedural range -- heavy, double, arc, diagonal, dashed, block, shade
/// and braille, and a red heavy corner -- which both arms must take from
/// baud's own masks rather than the face. The first box row ends in what a
/// text-drawn image is made of -- block elements, a space and a box-drawing
/// stroke on a set background, and a reversed block -- which the theme's
/// `AppBg::Opaque` keeps opaque, as it does the fourth row's `blue bg` and
/// `reverse` spans. That row ends on palette entries 0, 7, 8 and 15, and
/// the scene holds SGR, `38;5`, `48;5` and truecolour cells, so the
/// reverse-video scene draws every cell's exchange (DEC STD 070 § 5.6.3.2)
/// through both arms and not only the defaults. Both read it from one
/// resolved table, so this compares their drawing of it; the exchange's own
/// rule is gated in `baud-render/tests/reverse_video.rs`.
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
    /// The cursor guide's records, drawn under the glyphs.
    grounds: Vec<Instance>,
    /// The cursor guide both arms draw, or none.
    guide: Option<baud_render::GuideLook>,
    /// The badge both arms draw, or none.
    badge: Option<baud_render::BadgeLook>,
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
    /// The launch splash both arms draw, or none.
    splash: Option<SplashLook>,
    /// iTerm2's fireworks both arms draw: the shows in flight.
    fireworks: Vec<baud_render::fireworks::Show>,
    /// An input method's composition both arms draw, or none.
    composition: Option<Composition>,
    /// Row 9's predicted echoes, each a codepoint at its logical cell,
    /// which the cell arm draws as the app draws them; the instance arm
    /// draws none, as the app's instance and CPU paths draw none.
    predictions: Vec<(Row, Col, Codepoint)>,
    /// The find field and its marks both arms draw, or none.
    find: Option<baud_render::FindLook>,
    /// The link preview both arms draw, or none.
    link: Option<baud_render::LinkLook>,
    /// The lines at rest both arms draw, how, and the link under the pointer.
    lines: Vec<baud_render::LinkLine>,
    line_style: baud_render::LineStyle,
    over: Option<baud_render::LinkHover>,
    /// The hover tip both arms draw, or none.
    tip: Option<baud_render::TipLook>,
    /// How many of the cell arm's records, at the end, are baud's own
    /// overlays, drawn over the images.
    chrome: Tally,
    /// The window copied flat, in screen order -- research decision D4's
    /// buffer, and what the app binds. Held here so a third arm can draw
    /// from it and be compared against the two that read the live grid.
    copy: GridBuffer,
    copy_mem: &'static mut [PackedCell],
    /// The window in the order each row is drawn in, as the app lays it
    /// out (`Orders::window`): what the cell arm's walks read.
    bidi_cells: Vec<PackedCell>,
    bidi_used: Vec<baud_domain::RowExtent>,
    /// Smooth scroll: the pixels the screen is shifted down, or none. Both
    /// cell arms draw one extra leaving row above the window when set.
    slide: Option<Px>,
    /// Where the cell arm's resolved table stands: kept across scenes, as the
    /// app keeps a set's, so each scene's frame resolves only what it wrote
    /// or what an input moved -- the cache the app draws from.
    stamp: StyleStamp,
    /// Which of `records` are the trail's and the blink animation's.
    moving: (usize, usize),
    /// Which grid the cell arm reads: the core's, or the window copy as
    /// the app does -- what a frame with the config's notice row needs,
    /// which the core's grid has no row for.
    reads: Reads,
    /// Pixels the target is wider and taller than the frame, which both arms
    /// letterbox to its centre: a scene that sets it draws at a frame origin
    /// other than zero, which a fragment function that forgot the origin
    /// would misplace in both arms alike.
    margin: (u32, u32),
    /// The screen each cell frame is built from, as the app's record names
    /// it (`Screens::take`), over a copy of its own: what the GPU takes a
    /// frame's change against.
    screens: Screens,
    screen: Vec<PackedCell>,
    /// What the cell arm's last frame was built from, beyond its buffers
    /// and `records`, as the app keeps it for a screen switch's kept frame:
    /// how many records the walk wrote, each row's mark as it read it, the
    /// composition it walked and the ring the copy holds.
    walked: Tally,
    used: Vec<baud_domain::RowExtent>,
    walked_composition: Option<Composition>,
    ring: Rows,
}

/// Which grid the cell arm reads ([`Arms::reads`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Reads {
    Core,
    Copy,
}

/// Whether a frame counts toward the rows-drawn bound ([`Rotation::drew`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Counted {
    Yes,
    No,
}

impl Arms<'_, '_> {
    /// Both arms drawn offscreen and read back.
    fn draw(&mut self, t: &mut Term<'_>) -> (Vec<u32>, Vec<u32>) {
        // The whole frame: the exit-status strip, when kept, and the grid.
        let (fw, fh) = self.renderer.whole_size();
        let (w, h) = (
            Px::of(fw.value() + self.margin.0),
            Px::of(fh.value() + self.margin.1),
        );
        let n = usize::try_from(w.value() * h.value()).unwrap();
        let target_a = self.gpu.offscreen(w, h).expect("target");
        let target_b = self.gpu.offscreen(w, h).expect("target");
        // The cell arm first: it reads the grid under its rotation, and the
        // instance arm's `convert` would turn it in place.
        let b = self.with_cells(t, |gpu, renderer, input| {
            let Some(Drawn::Frame(_)) = gpu.draw_cells(renderer, input, &target_b, Done::Wait)
            else {
                panic!("draw_cells");
            };
            let mut b = vec![0u32; n];
            gpu.read(&target_b, &mut b).expect("read");
            b
        });
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

    /// **The glyph map warmed by the cell arm alone**, as the app's cell
    /// path draws, which converts nothing: a line of Latin in the default
    /// rendition, so a scene drawn next on a fresh renderer finds the map
    /// in use (`Arms::draw`) and the renderer's own rendition table still
    /// unresolved, as the cell arm's first frame meets it.
    fn warm(&mut self, t: &mut Term<'_>) {
        feed(
            t,
            b"\x1bc\x1b[?25lThe quick brown fox jumps over the lazy dog 0123456789",
        );
        let (w, h) = self.target_size();
        let target = self.gpu.offscreen(w, h).expect("target");
        self.with_cells(t, |gpu, renderer, input| {
            let Some(Drawn::Frame(_)) = gpu.draw_cells(renderer, input, &target, Done::Wait) else {
                panic!("draw_cells");
            };
        });
    }

    /// The size `draw` draws its targets at.
    fn target_size(&self) -> (Px, Px) {
        let (fw, fh) = self.renderer.whole_size();
        (
            Px::of(fw.value() + self.margin.0),
            Px::of(fh.value() + self.margin.1),
        )
    }

    /// **Keeps the old screen for a screen switch's slide on the instance
    /// path**, as the app keeps it once the switch is read: the rows under
    /// the cursor converted again from the renderer's copy of the last
    /// frame, and that frame drawn whole into the GPU's kept texture.
    fn keep_instances(&mut self, t: &Term<'_>) {
        self.renderer.hide_cursor(t);
        let size = self.target_size();
        let Some(Drawn::Frame(_)) = self.gpu.draw_kept(&mut self.renderer, size, Done::Wait) else {
            panic!("the instance arm's kept frame");
        };
    }

    /// **Keeps the old screen on the cell path**, as `App::draw_kept`
    /// builds it once the switch is read: the last frame's window, copied
    /// (`Reads::Copy`), read as a view of its own over the marks the walk
    /// read, walked again with no cursor, the records after the walk's the
    /// last frame's but the trail's and the blink animation's, the grounds
    /// the badge alone, and drawn whole into the GPU's kept texture.
    fn keep_cells(&mut self, t: &Term<'_>) {
        assert_eq!(self.reads, Reads::Copy, "the kept frame reads the copy");
        let (cols, rows) = (self.renderer.grid().0, Rows::of_count(self.used.len()));
        let stride = t.stride();
        let n = rows.index() * stride.index();
        let view = baud_domain::Viewport::new(
            &self.copy_mem[..n],
            &self.used,
            stride,
            cols,
            rows,
            rows,
            RowRot::NONE,
        );
        self.renderer.set_trail(None);
        self.renderer.set_cursor_glow_at(None);
        self.renderer.place_grounds(None, None);
        let mut records = Vec::new();
        self.exceptions.ensure(
            &Scene {
                term: t,
                view,
                styles: self.styles_mem,
                cursor: None,
                highlight: self.highlight,
                map: &self.map,
                candidates: &self.candidates,
                pan: self.slide.unwrap_or(Px::of(0)),
                composition: self.walked_composition.as_ref(),
            },
            &mut self.renderer,
            self.overlay_mem,
            &mut records,
        );
        let under = self.exceptions.under();
        let walked = usize::try_from(self.walked.value()).unwrap();
        let tail = &self.records[walked..];
        let (ma, mb) = (self.moving.0 - walked, self.moving.1 - walked);
        let moving = records.len() + ma;
        records.extend_from_slice(&tail[..ma]);
        records.extend_from_slice(&tail[mb..]);
        let grounds = self.renderer.badge().to_vec();
        let input = CellInput {
            set: baud_present_macos::journal::SetIx(0),
            grid: &self.copy,
            styles: &self.styles,
            map: &self.map_buffer,
            stride,
            rotation: RowRot::NONE,
            ring: self.ring,
            cursor: None,
            cursor_drawn: self.exceptions.cursor_drawn(),
            highlight: self.highlight,
            overlay: &self.overlay,
            records: &records,
            grounds: &grounds,
            under,
            chrome: self.chrome,
            slide: self.slide,
            table: self.stamp,
            map_gen: self.map.generation(),
            reach: self.map.reach(),
            moving: (moving, moving),
            screen: self.screens.now(),
        };
        let size = self.target_size();
        let Some(Drawn::Frame(_)) =
            self.gpu
                .draw_cells_kept(&mut self.renderer, &input, size, Done::Wait)
        else {
            panic!("the cell arm's kept frame");
        };
    }

    /// The cell arm's frame built as the app builds it -- the styles synced,
    /// the map and the overlay walked, the records appended in the app's
    /// order -- and handed to `f` with the GPU and the renderer to draw.
    fn with_cells<R>(
        &mut self,
        t: &mut Term<'_>,
        f: impl FnOnce(&mut Gpu, &mut Renderer<'_, '_>, &CellInput<'_>) -> R,
    ) -> R {
        // The window in the order each row is drawn in, laid out for this
        // frame as the app lays it out: the core's own view when no row is
        // reordered.
        // The composition and the predicted echoes, on the live view, laid
        // out as written: set before the layout, as the app sets them.
        self.renderer.set_composition(self.composition.clone());
        let joins_seen = self.renderer.joins_seen();
        let live_back = self.back.index() == 0;
        self.renderer
            .set_predicted(self.predictions.iter().copied().filter(|_| live_back));
        let rejoin = self.renderer.lay_out_bidi_copied(t, self.back);
        let view =
            self.renderer
                .bidi()
                .window(t, self.back, &mut self.bidi_cells, &mut self.bidi_used);
        self.renderer.set_highlight(self.highlight);
        self.renderer.set_cursor_guide(self.guide);
        self.renderer.set_badge(self.badge.clone());
        self.renderer.set_scrollbar(self.window);
        self.renderer.set_trail(self.trail);
        self.renderer.set_screen_fade(self.fade);
        self.renderer.set_splash(self.splash);
        // Over none of what the shell drew, as each driver hands it.
        self.renderer
            .splash_yields_to(t, self.slide.unwrap_or(Px::of(0)));
        self.renderer.set_fireworks(&self.fireworks);
        self.renderer.set_composition(self.composition.clone());
        self.renderer.set_find(t, self.find.clone());
        self.renderer.set_link(self.link.clone());
        let lines = core::mem::take(&mut self.lines);
        self.renderer.set_link_lines(&lines, self.line_style);
        self.lines = lines;
        self.renderer.set_link_hover(self.over.as_ref());
        self.renderer.set_tip(self.tip.clone());
        // The exit-status marks on this window, as the app places them for
        // every path; none while the setting is off.
        self.renderer
            .place_marks(t, self.back, baud_render::Leaving::None, Px::of(0));
        // And the activity lights on their own line, as the app places them
        // for every path; none without the line.
        self.renderer.place_lights(t);
        // Both arms draw the same image placements: computed once here, read by
        // `draw` and `draw_cells` alike.
        self.renderer.set_images(t, self.back);
        // The styles before the map walk, as the app orders them: the walk
        // reads each cell's face style from the synced table.
        self.renderer
            .sync_styles(t, self.styles_mem, &mut self.stamp);
        // Whether this frame's screen is the last's, as the app decides it:
        // the window as drawn, its status line and line renditions.
        self.screens.compare = Compare::for_rejoin(rejoin);
        self.screens.take(
            &mut [&mut self.screen[..]],
            SetIx(0),
            &view,
            t.status_line().as_ref(),
            t.stride(),
            match self.slide {
                Some(_) => baud_render::Leaving::Drawn,
                None => baud_render::Leaving::None,
            },
        );
        // The kept joins found again where that compare found a row changed,
        // before the walk and the records, as the app finds them.
        if let Some(how) = rejoin {
            let screens = &self.screens;
            self.renderer
                .rejoin_bidi(t, self.back, how, |r| screens.changed(r));
        }
        // The app's rule: the window by the core's clamp, so the alternate
        // screen -- which keeps no history -- has its cursor whatever
        // window the scene carries.
        let live = t.looking_back(self.back).index() == 0;
        let composition = self.composition.clone().filter(|_| live);
        let shown = status_cursor(t, view.rows()).filter(|_| live).or_else(|| {
            (t.cursor_visible() && live)
                .then(|| match &composition {
                    Some(c) => c.cursor(c.width_on(&view)),
                    None => Some(t.cursor()),
                })
                .flatten()
                .map(|(r, c)| (r, c, t.cursor_style()))
        });
        // The drawn cursor after the last predicted echo, as the app puts it.
        let shown = match (shown, self.predictions.last().filter(|_| live_back)) {
            (Some((_, _, style)), Some(&(row, col, _))) => Some((row, col.after(), style)),
            (c, _) => c,
        };
        // No cursor over an address or a message, as the app has it.
        let shown = self.renderer.clear_of_messages(t, shown);
        // A slide's leaving row, as the shader reads it, walked into the map
        // before the window, as the app walks it.
        if self.slide.is_some() {
            let cols = view.cols().index();
            let mut row = vec![PackedCell::EMPTY; cols];
            self.renderer.copy_leaving(t, self.back, &mut row);
            let used = [baud_domain::RowExtent::of(cols)];
            let leaving = baud_domain::Viewport::new(
                &row,
                &used,
                baud_domain::Stride::of_cells(cols),
                view.cols(),
                Rows::of_count(1),
                Rows::of_count(1),
                RowRot::NONE,
            );
            self.exceptions.ensure_leaving(
                &mut self.map,
                t,
                leaving,
                self.styles_mem,
                &mut self.renderer,
            );
        }
        self.map.ensure(
            t,
            view,
            self.styles_mem,
            &mut self.renderer,
            &mut self.candidates,
        );
        // Laid out again once the walk shaped the renderer's first letter
        // that takes a joining form with BDSM reset, as the app does.
        self.renderer.join_after_walk(t, self.back, joins_seen);
        // The lines at rest and the look of the link under the pointer, as
        // the app places them: on the final layout, before the walk, which
        // reads the look's fields as empty.
        self.renderer.place_links(t, &view);
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
        self.walked = Tally::of(self.records.len());
        self.used.clear();
        self.used
            .extend((0..view.rows().index()).map(|y| view.extent(Row::from_index(y))));
        self.walked_composition = composition.clone();
        // The predicted echoes as the app draws them on the cell path: each
        // a masked cell and a record, in the rendition of the logical cell
        // to its left.
        if live_back {
            for &(row, col, cp) in &self.predictions {
                let at = col.index().saturating_sub(1);
                let style = t
                    .view(self.back)
                    .row(row)
                    .get(at)
                    .map_or(PackedCell::EMPTY.style, |c| c.style);
                self.exceptions.predicted(
                    &mut self.renderer,
                    (row, col),
                    PackedCell {
                        content: baud_domain::Content::codepoint(cp),
                        style,
                        flags: baud_domain::CellFlags::NONE,
                    },
                    self.styles_mem,
                    cursor,
                    self.highlight,
                    view.cols(),
                    self.overlay_mem,
                    &mut self.records,
                );
            }
        }
        // The composition as the app draws it on the cell path: its cells
        // masked, its records after the walk's.
        if let Some(c) = &composition {
            let under = view
                .row(c.row)
                .get(self.renderer.bidi().visual_col(c.row, c.col).index())
                .copied()
                .unwrap_or(PackedCell::EMPTY);
            self.exceptions.composed(
                &mut self.renderer,
                t,
                (c, baud_render::Laid::AsWritten),
                under,
                self.styles_mem,
                cursor,
                view.cols(),
                view.rendition(c.row),
                self.slide.unwrap_or(Px::of(0)),
                self.overlay_mem,
                &mut self.records,
            );
        }
        // The find's marks and the link preview's underline as the app
        // draws them on the cell path, with the content.
        self.records.extend_from_slice(self.renderer.find_solids());
        self.records.extend_from_slice(self.renderer.link_solids());
        // The lines at rest, placed before the walk, as the app draws them.
        self.renderer
            .link_lines_lowered(self.slide.unwrap_or(Px::of(0)), &mut self.records);
        // The leaving row of a slide as the app draws it: masked after the
        // screen's rows where records draw it. Only where the history holds
        // one.
        if let Some(pan) = self.slide {
            let above = Rows::of_index(self.back.index() + 1);
            if t.looking_back(above) == above {
                self.exceptions.leaving(
                    &mut self.renderer,
                    t,
                    (&self.map, self.styles_mem),
                    self.back,
                    (view.cols(), view.rows()),
                    pan,
                    self.overlay_mem,
                    &mut self.records,
                );
            }
        }
        // The status line's records and its row of the mask, as the app
        // adds them: nothing while no line is shown.
        self.exceptions.status(
            &mut self.renderer,
            t,
            cursor,
            (view.cols(), view.rows()),
            self.overlay_mem,
            &mut self.records,
        );
        self.moving.0 = self.records.len();
        self.records.extend_from_slice(self.renderer.trail());
        // The plasma blink's glow, from the cursor before the phase's
        // filter, as the app builds it.
        self.renderer.set_cursor_glow_at(shown);
        self.records.extend_from_slice(self.renderer.cursor_glow());
        self.moving.1 = self.records.len();
        // The cursor guide as the app draws it on the cell path: placed from
        // the live cursor, else on the cursor's line while the window looks
        // back, its records under the glyphs.
        self.renderer
            .place_grounds(shown, Renderer::cursor_line(t, self.back));
        self.grounds.clear();
        self.grounds.extend_from_slice(self.renderer.badge());
        self.renderer
            .cursor_guide_lowered(self.slide.unwrap_or(Px::of(0)), &mut self.grounds);
        // baud's own overlays last, over the images, as the app draws
        // them: a `BEL`'s tint, the find field's cells masked and its
        // records and caret, the link preview's bar the same way, the
        // thumb, the fade.
        let chrome_from = self.records.len();
        self.records.extend_from_slice(self.renderer.bell_tint());
        for (c, under) in self.renderer.hover_bars() {
            self.exceptions.composed(
                &mut self.renderer,
                t,
                (&c, baud_render::Laid::OnScreen),
                under,
                self.styles_mem,
                None,
                view.cols(),
                baud_domain::LineRendition::Single,
                Px::of(0),
                self.overlay_mem,
                &mut self.records,
            );
        }
        let tips = self
            .renderer
            .tip_bars()
            .into_iter()
            .map(|c| (Some(c), false));
        let fields = [
            (self.renderer.find_field(), true),
            (self.renderer.link_bar(), false),
        ]
        .into_iter()
        .chain(tips)
        .chain([(self.renderer.notice_bar(), false)]);
        for (c, caret) in fields {
            let Some(c) = c else { continue };
            self.exceptions.composed(
                &mut self.renderer,
                t,
                (&c, baud_render::Laid::OnScreen),
                PackedCell::EMPTY,
                self.styles_mem,
                None,
                view.cols(),
                baud_domain::LineRendition::Single,
                Px::of(0),
                self.overlay_mem,
                &mut self.records,
            );
            if caret {
                self.records.extend_from_slice(self.renderer.find_caret());
            }
        }
        self.records.extend_from_slice(self.renderer.splash());
        self.records.extend_from_slice(self.renderer.scrollbar());
        self.records.extend_from_slice(self.renderer.screen_fade());
        self.chrome = Tally::of(self.records.len() - chrome_from);
        let (grid, rotation, ring) = match self.reads {
            Reads::Copy => {
                let ring = copy_window(t, self.back, self.slide, &mut self.renderer, self.copy_mem);
                (&self.copy, RowRot::NONE, Rows::of_index(ring))
            }
            Reads::Core => (&self.grid, view.rotation(), t.ring()),
        };
        self.ring = ring;
        let input = CellInput {
            set: baud_present_macos::journal::SetIx(0),
            grid,
            styles: &self.styles,
            map: &self.map_buffer,
            stride: t.stride(),
            rotation,
            ring,
            cursor,
            cursor_drawn: self.exceptions.cursor_drawn(),
            highlight: self.highlight,
            overlay: &self.overlay,
            records: &self.records,
            grounds: &self.grounds,
            under: self.exceptions.under(),
            chrome: self.chrome,
            // In place: the shared ring holds the leaving row one row up
            // (ring row `rot - 1`), reached by the shader's own modulo.
            slide: self.slide,
            table: self.stamp,
            map_gen: self.map.generation(),
            reach: self.map.reach(),
            moving: self.moving,
            screen: self.screens.now(),
        };
        f(&mut self.gpu, &mut self.renderer, &input)
    }

    /// The cell path again, reading **a copy of the window** rather than
    /// the core's grid: D4's buffer, laid out flat in screen order, so the
    /// shader's rotation is the identity and its ring is the screen.
    ///
    /// Everything else is the frame `draw` just built -- the same styles,
    /// the same map, the same overlay and records -- so a difference in
    /// the pixels can only be the grid it read.
    fn draw_copied(&mut self, t: &mut Term<'_>) -> Vec<u32> {
        // The whole frame, as `draw` draws it: the grid's frame and what
        // is kept beside it, the strip and the margins.
        let (w, h) = self.renderer.whole_size();
        let n = usize::try_from(w.value() * h.value()).unwrap();
        let target = self.gpu.offscreen(w, h).expect("target");
        let view = t.view(self.back);
        self.renderer.set_images(t, self.back);
        self.renderer.lay_out_bidi(t, self.back);
        let ring = copy_window(t, self.back, self.slide, &mut self.renderer, self.copy_mem);
        // The app's rule: the window by the core's clamp, so the alternate
        // screen -- which keeps no history -- has its cursor whatever
        // window the scene carries.
        let live = t.looking_back(self.back).index() == 0;
        let cursor = status_cursor(t, view.rows())
            .filter(|_| live)
            .or_else(|| {
                (t.cursor_visible() && live)
                    .then(|| match self.composition.as_ref().filter(|_| live) {
                        Some(c) => c.cursor(c.width_on(&view)),
                        None => Some(t.cursor()),
                    })
                    .flatten()
                    .map(|(r, c)| (r, c, t.cursor_style()))
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
            cursor_drawn: self.exceptions.cursor_drawn(),
            highlight: self.highlight,
            overlay: &self.overlay,
            records: &self.records,
            grounds: &self.grounds,
            under: self.exceptions.under(),
            chrome: self.chrome,
            slide: self.slide,
            table: self.stamp,
            map_gen: self.map.generation(),
            reach: self.map.reach(),
            moving: self.moving,
            // The screen `draw` just built from.
            screen: self.screens.now(),
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

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// **The fire's reference**: `fire_flame`, through the renderer's own Rust
/// model of it, `fire::light` -- which the CPU path draws by, so the oracle
/// ties the shader and the CPU arm to one model.
fn fire_reference(vfx: &Vfx, p: Pt, color: [f32; 4]) -> [f32; 4] {
    let a = color[3] * fire::light(vfx, p);
    [color[0] * a, color[1] * a, color[2] * a, a]
}

/// The plasma's parts at `p`, through `plasma::parts`: the envelope, the
/// tube with its sheath, the filaments, and how far behind the head the
/// pixel is along the axis in cells -- `None` where the fragment function
/// draws nothing.
fn plasma_parts(vfx: &Vfx, p: Pt) -> Option<(f32, f32, f32, f32)> {
    plasma::parts(vfx, p).map(|q| (q.env, q.tube, q.filaments, q.behind))
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

/// **The plasma's reference**: `plasma_tube` for the pixel centred at `p`,
/// its light meeting the record's alpha by the frame's cap, `Vfx::cap`.
fn plasma_reference(vfx: &Vfx, p: Pt, color: [f32; 4]) -> [f32; 4] {
    let Some((env, tube, fil, _)) = plasma_parts(vfx, p) else {
        return [0.0; 4];
    };
    let light = (env * (tube + (1.0 - tube) * fil)).clamp(0.0, 1.0);
    let a = vfx.cap().of(light, color[3]);
    [color[0] * a, color[1] * a, color[2] * a, a]
}

/// The plasma's light scaled by the record's alpha whatever the frame's
/// cap: a repeat pass as every effect drew one before the clamp.
fn plasma_scaled(vfx: &Vfx, p: Pt, color: [f32; 4]) -> [f32; 4] {
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
    mixed(src, dst, Mix::Normal)
}

/// A channel of premultiplied `s` over `d` by `mix`, the backdrop's alpha
/// `ab` and the source's `a`: W3C Compositing's section 6 in premultiplied
/// numbers, `cs + cb (1 - as)` source-over, `cs + cb - cs cb` by `screen`
/// and `cs (1 - ab) + cb (1 - as + cs)` by `multiply`, as the blend states
/// form them.
fn channel(s: f32, d: f32, (a, ab): (f32, f32), mix: Mix) -> f32 {
    match mix {
        Mix::Normal => s + d * (1.0 - a),
        Mix::Screen => s + d * (1.0 - s),
        Mix::Multiply => s * (1.0 - ab) + d * (1.0 - a + s),
    }
}

/// Premultiplied `src` over the stored pixel `dst` by `mix`, rounded to the
/// target's bytes the way a blend unit stores them; the alpha `as + ab (1 -
/// as)` under every mode.
fn mixed(src: [f32; 4], dst: u32, mix: Mix) -> u32 {
    let ch = |k: u32| f32::from(u8::try_from((dst >> k) & 0xff).unwrap_or(0)) / 255.0;
    let (r, g, b, a) = (ch(16), ch(8), ch(0), ch(24));
    let byte = |v: f32| u32::try_from((v * 255.0).round().clamp(0.0, 255.0) as i64).unwrap_or(0);
    let c = |s: f32, d: f32| byte(channel(s, d, (src[3], a), mix));
    (byte(src[3] + a * (1.0 - src[3])) << 24)
        | (c(src[0], r) << 16)
        | (c(src[1], g) << 8)
        | c(src[2], b)
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
                    strength: baud_render::Strength::Full,
                    opacity: Smear::OPAQUE,
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
    (width, origin): (Px, (usize, usize)),
    mix: Mix,
) -> (Vec<u32>, Vec<bool>) {
    effect_frame_by(
        reference(vfx.effect),
        vfx,
        records,
        plain,
        (width, origin),
        mix,
    )
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
    (width, origin): (Px, (usize, usize)),
    mix: Mix,
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
                ties[at] = on_tie(src, out[at], mix);
                out[at] = mixed(src, out[at], mix);
            }
        }
    }
    (out, ties)
}

/// Whether `src` over `dst` lands within a hundredth of a step of halfway
/// between two bytes in any channel: a tie the blend unit may round either
/// way. A flat patch of one value on a tie rounds apart as a block -- the
/// ufo's hull did, 63 pixels at once.
fn on_tie(src: [f32; 4], dst: u32, mix: Mix) -> bool {
    let ch = |k: u32| f32::from(u8::try_from((dst >> k) & 0xff).unwrap_or(0)) / 255.0;
    let (r, g, b, a) = (ch(16), ch(8), ch(0), ch(24));
    let near = |v: f32| {
        let v = v * 255.0;
        ((v - v.floor()) - 0.5).abs() < 0.01
    };
    let c = |s: f32, d: f32| near(channel(s, d, (src[3], a), mix));
    near(src[3] + a * (1.0 - src[3])) || c(src[0], r) || c(src[1], g) || c(src[2], b)
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
        // anchored on the frame rather than on the path -- differs. The
        // leftward scene is the move across taken back: the only one whose
        // ignition moves left, so the only one where a plasma shader that
        // ignored the ignition's hand (`plasma::Hand`) differs.
        let across = (cell_at(98.0, 2.0), cell_at(116.0, 3.0));
        let down = (cell_at(104.0, 0.0), cell_at(106.0, 8.0));
        let line = (bar(cell_at(98.0, 4.0)), bar(cell_at(116.0, 4.0)));
        let long = (cell_at(60.0, 2.0), cell_at(116.0, 3.0));
        let back = (across.1, across.0);
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
            ("leftward", back, back, 1.0, Seed(7)),
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
                        strength: baud_render::Strength::Full,
                        opacity: Smear::OPAQUE,
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
                let records = arms.renderer.trail().to_vec();
                let records = &records[..];
                let mix = arms.renderer.trail_mix();
                let (want, ties) = effect_frame(&vfx, records, &plain, (width, origin), mix);
                // The CPU arm, which the Linux driver draws with: the same
                // records by the same models over its own frame, which is
                // the GPU's plain frame -- so every pixel the reference's.
                let cpu = cpu_frame(arms, t);
                let fw = usize::try_from(arms.renderer.whole_size().0.value()).unwrap();
                let cpu_off = cpu
                    .iter()
                    .enumerate()
                    .filter(|&(i, &p)| p != want[(i / fw + origin.1) * tw + i % fw + origin.0])
                    .count();
                assert_eq!(cpu_off, 0, "{at}: the CPU arm is off the reference");
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
                        (width, origin),
                        mix,
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
    // **The plasma's repeat pass is clamped at its cap on every path**:
    // across the screen at two ages, drawn as a repeat, both GPU arms and
    // the CPU arm are the reference's -- each band's light clamped at the
    // cap its record carries, `min(light, cap)` -- and a hundred pixels or
    // more are off the light scaled by the cap, which the clamp replaced.
    // Red on a shader that scales.
    let (from, to) = (cell_at(98.0, 2.0), cell_at(116.0, 3.0));
    let ignite = Ignition::ignite(centre(from), to, Seed(7), ch);
    let seg = Segment::new(from.corners(), to);
    for age in [0.03f32, 0.08] {
        let vfx = Vfx {
            effect: Effect::Plasma,
            emitter: Smear {
                corners: seg.at(age),
                hole: to,
                shape: Shape::Quad,
                strength: baud_render::Strength::UnderThreshold,
                opacity: Smear::OPAQUE,
            },
            ignition: ignite.at(age),
            axis: ignite.dir,
        };
        let at = format!("the plasma's repeat at {age}");
        arms.trail = Some(TrailLook::Vfx(vfx));
        // The cap is scanned over the frames after the first.
        let _ = arms.draw(t);
        let (a, b) = arms.draw(t);
        assert_eq!(differing(&a, &b), 0, "the arms disagree under {at}");
        let records = arms.renderer.trail().to_vec();
        let cap = arms.renderer.flash_cap();
        assert!(
            cap.get() > 0 && cap.get() < 0xFF,
            "{at}: the control, a cap that draws and holds back: {}",
            cap.get()
        );
        assert!(
            !records.is_empty()
                && records
                    .iter()
                    .all(|r| r.kind == Kind::Band && r.color.a() == cap),
            "{at}: bands alone, each at the cap"
        );
        let mix = arms.renderer.trail_mix();
        let (want, _) = effect_frame(&vfx, &records, &plain, (width, origin), mix);
        let cpu = cpu_frame(arms, t);
        let fw = usize::try_from(arms.renderer.whole_size().0.value()).unwrap();
        let cpu_off = cpu
            .iter()
            .enumerate()
            .filter(|&(i, &p)| p != want[(i / fw + origin.1) * tw + i % fw + origin.0])
            .count();
        assert_eq!(cpu_off, 0, "{at}: the CPU arm is off the reference");
        let ch = |p: u32, k: u32| (p >> (8 * k)) & 0xff;
        let step = |x: u32, y: u32| (0..4).map(|k| ch(x, k).abs_diff(ch(y, k))).max().unwrap();
        for (i, (&x, &y)) in a.iter().zip(&want).enumerate() {
            assert!(
                step(x, y) <= 1,
                "{at}: pixel {i} is {x:08x}, the reference {y:08x}"
            );
        }
        let (scaled, _) = effect_frame_by(
            Reference {
                band: plasma_scaled,
                particle: fx_particle,
            },
            &vfx,
            &records,
            &plain,
            (width, origin),
            mix,
        );
        let brighter = a
            .iter()
            .zip(&scaled)
            .filter(|&(&x, &y)| step(x, y) > 1)
            .count();
        assert!(
            brighter > 100,
            "{at}: only {brighter} pixels off the scaled light, so the clamp shows nothing"
        );
    }
    // **The plasma in front of the text on every page, on both GPU paths**:
    // along the scene's first row, over its text in the default colour, at
    // two ages on the dark theme and the light, each opaque and on the glass
    // at the default dims, both GPU arms are the
    // reference by the renderer's mix -- `screen` on the dark theme,
    // `multiply` on the light, the glass's `multiply` through the second
    // source -- within a step, the CPU arm exactly, and a hundred pixels or
    // more are off the same records drawn source-over, which the mix
    // replaced. Red on a pipeline that draws the plasma source-over, or a
    // `multiply` without the glass's `cs (1 - ab)`.
    let resting = arms.renderer.theme().clone();
    // Along the first row, over its text in the default colour.
    let (from, to) = (cell_at(2.0, 0.0), cell_at(40.0, 0.0));
    let ignite = Ignition::ignite(centre(from), to, Seed(7), ch);
    let seg = Segment::new(from.corners(), to);
    for (half, theme, mix) in [
        ("dark", Theme::dark(), Mix::Screen),
        (
            "dark glass",
            Theme {
                bg_alpha: Chan::from_u8(179),
                ..Theme::dark()
            },
            Mix::Screen,
        ),
        ("light", Theme::light(), Mix::Multiply),
        (
            "light glass",
            Theme {
                bg_alpha: Chan::from_u8(153),
                ..Theme::light()
            },
            Mix::Multiply,
        ),
    ] {
        arms.renderer.set_theme(theme);
        arms.trail = None;
        let (plain, _) = arms.draw(t);
        for age in [0.03f32, 0.1] {
            let vfx = Vfx {
                effect: Effect::Plasma,
                emitter: Smear {
                    corners: seg.at(age),
                    hole: to,
                    shape: Shape::Quad,
                    strength: baud_render::Strength::Full,
                    opacity: Smear::OPAQUE,
                },
                ignition: ignite.at(age),
                axis: ignite.dir,
            };
            let at = format!("the plasma in front, {half}, at {age}");
            arms.trail = Some(TrailLook::Vfx(vfx));
            let (a, b) = arms.draw(t);
            assert_eq!(differing(&a, &b), 0, "the arms disagree under {at}");
            assert_eq!(arms.renderer.trail_mix(), mix, "{at}: the mix");
            let records = arms.renderer.trail().to_vec();
            let (want, ties) = effect_frame(&vfx, &records, &plain, (width, origin), mix);
            let cpu = cpu_frame(arms, t);
            let fw = usize::try_from(arms.renderer.whole_size().0.value()).unwrap();
            let cpu_off = cpu
                .iter()
                .enumerate()
                .filter(|&(i, &p)| p != want[(i / fw + origin.1) * tw + i % fw + origin.0])
                .count();
            assert_eq!(cpu_off, 0, "{at}: the CPU arm is off the reference");
            let ch = |p: u32, k: u32| (p >> (8 * k)) & 0xff;
            let step = |x: u32, y: u32| (0..4).map(|k| ch(x, k).abs_diff(ch(y, k))).max().unwrap();
            let mut off = 0usize;
            for (i, (&x, &y)) in a.iter().zip(&want).enumerate() {
                assert!(
                    step(x, y) <= 1,
                    "{at}: pixel {i} is {x:08x}, the reference {y:08x}"
                );
                off += usize::from(step(x, y) == 1 && !ties[i]);
                assert!(
                    (0..3).all(|k| ch(x, k) <= ch(x, 3)),
                    "{at}: pixel {i} {x:08x} stores colour past its alpha"
                );
            }
            assert!(off <= 20, "{at}: {off} pixels a step off the reference");
            let (over, _) = effect_frame(&vfx, &records, &plain, (width, origin), Mix::Normal);
            let moved = a
                .iter()
                .zip(&over)
                .filter(|&(&x, &y)| step(x, y) > 2)
                .count();
            assert!(
                moved > 100,
                "{at}: only {moved} pixels off source-over, so the mix shows nothing"
            );
        }
    }
    arms.renderer.set_theme(resting);
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

/// The status line's cursor, on the row after the screen's `rows`, while
/// DECSASD selects the line: the app's rule (`live_cursor`).
fn status_cursor(t: &Term<'_>, rows: Rows) -> Option<(Row, Col, baud_domain::CursorStyle)> {
    t.status_line()
        .and_then(|l| l.cursor)
        .map(|c| (Row::from_index(rows.index()), c, t.cursor_style()))
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

/// Both GPU arms over `$renderer`, on the scene's grid, bound to `$arms`
/// and `$t` in the caller's scope: the terminal whose grid is the GPU's
/// shared memory, the core writing it and the shader reading it.
macro_rules! arms {
    // With image memory: without it a stored Sixel is *refused*, both arms
    // draw nothing, and the image comparison passes while proving the opposite
    // -- the `golden.rs` trap this file exists not to fall into.
    ($renderer:expr => $arms:ident, $t:ident) => {
        arms!($renderer => $arms, $t, ImageBudget::of(64 * 48 + 64))
    };
    // A budget of the caller's: what a picture larger than the swirl needs.
    ($renderer:expr => $arms:ident, $t:ident, $budget:expr) => {
        let gpu = Gpu::new().expect("a Metal device; this test cannot skip itself");
        // The viewport is the GPU's memory: the core writes it, the shader
        // reads it.
        let mut owned = Owned::without_surfaces(&WS, &CacheGeometry::UNKNOWN, Depth::of(12))
            .with_images($budget);
        let (grid, cells) = gpu
            .grid(owned.ring(), owned.stride())
            .expect("a shared grid");
        let (_, alt) = gpu
            .grid(owned.rows(), owned.stride())
            .expect("a shared alternate");
        let owned_stride = owned.stride();
        let mut $t = Term::new(owned.storage_over(cells, alt), &WS);
        let (styles, styles_mem) = gpu.styles(&$t).expect("shared styles");
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
        let mut $arms = Arms {
            gpu,
            renderer: $renderer,
            grid,
            styles,
            styles_mem,
            map_buffer,
            map: GlyphMap::over(map_mem),
            overlay,
            overlay_mem,
            exceptions: Overlay::new(),
            records: Vec::new(),
            grounds: Vec::new(),
            guide: None,
            badge: None,
            candidates: Vec::new(),
            back: Rows::ZERO,
            highlight: None,
            window: None,
            trail: None,
            fade: None,
            splash: None,
            fireworks: Vec::new(),
            composition: None,
            predictions: Vec::new(),
            find: None,
            link: None,
            lines: Vec::new(),
            line_style: baud_render::LineStyle::Solid,
            over: None,
            tip: None,
            chrome: Tally::default(),
            copy,
            copy_mem,
            bidi_cells: Vec::new(),
            bidi_used: Vec::new(),
            slide: None,
            stamp: StyleStamp::default(),
            moving: (0, 0),
            reads: Reads::Core,
            margin: (0, 0),
            screens: Screens::new(),
            screen: vec![PackedCell::EMPTY; owned_stride.index() * (usize::from(WS.ws_row) + 1)],
            walked: Tally::default(),
            used: Vec::new(),
            walked_composition: None,
            ring: Rows::ZERO,
        };
    };
}

/// **The alternate-screen slide draws alike on every arm**: entering the
/// alternate screen and leaving it, on the dark and the light theme, with
/// the frame at the target's origin and letterboxed by an odd margin, so
/// the frame starts half a pixel in. The two GPU arms draw the same pixels,
/// and each draws what `compose_slide` makes of its own unslid frame of the
/// new screen and the frame kept before the switch -- the margin from the
/// new frame, the new screen where the slide has brought it, the kept one
/// under the rest -- and with no margin the CPU arm's slide is theirs,
/// pixel for pixel.
/// The new screen is drawn under the slide's faint veil, as a driver
/// draws it. The old screen is kept by either GPU path's own rule once the
/// switch is read, and is that screen drawn with its cursor hidden
/// (`?25l`), though the frame shown before the switch drew a blinking
/// block on a letter carrying a combining mark, its glow lit, the cursor
/// guide on its row and a trail sweeping to it.
/// The controls: the frame shown drew the cursor, and the slid frame is
/// neither the new screen nor the kept one.
#[test]
fn the_slide_draws_alike_on_every_arm() {
    let font = |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let regular = font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&regular).unwrap()];
    let renderer = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    arms!(renderer => arms, t);
    // The window's copy, as the app binds it: the alternate screen is not
    // in the shared ring's memory.
    arms.reads = Reads::Copy;
    // A blinking block on the `i` that carries U+0301, on the shell's
    // third row: a cell the cell path draws by a record.
    let shell = b"\x1b[1 q\x1b[32muser@host\x1b[0m:~$ ls\r\nbaud-app  baud-core  \x1b[44mdocs\x1b[0m\r\n$ vi\xcc\x81m\r\n\x1b[3;4H";
    feed(&mut t, shell);
    let program = b"\x1b[H\x1b[2J\x1b[33m  1 \x1b[0mfn main() {}\r\n\x1b[34m~\x1b[0m\r\n\x1b[10;1H\x1b[7m main.rs \x1b[0m\x1b[1;5H";
    let band =
        |frame: &[u32], w: usize, top: usize, h: usize| frame[top * w..(top + h) * w].to_vec();
    let glowing = CursorPhase {
        glow: Chan::from_u8(160),
        ..CursorPhase::SHOWN
    };
    arms.guide = Some(baud_render::GuideLook {
        colour: baud_render::CURSOR_GUIDE,
        line: Px::of(1),
    });
    arms.renderer.set_cursor_blink(glowing);
    for (half, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
        arms.renderer.set_theme(theme);
        for margin in [(0, 0), (7, 5)] {
            arms.margin = margin;
            for (keeper, way, toward, switch) in [
                ("instances", "enter", Toward::Alternate, &b"\x1b[?1049h"[..]),
                ("instances", "leave", Toward::Primary, &b"\x1b[?1049l"[..]),
                ("cells", "enter", Toward::Alternate, &b"\x1b[?1049h"[..]),
                ("cells", "leave", Toward::Primary, &b"\x1b[?1049l"[..]),
            ] {
                let at = format!("{half}, margin {margin:?}, {keeper}, {way}");
                // The old screen with its cursor hidden: what is kept.
                feed(&mut t, b"\x1b[?25l");
                let (kept_a, kept_b) = arms.draw(&mut t);
                assert_eq!(
                    differing(&kept_a, &kept_b),
                    0,
                    "{at}: the kept frames differ"
                );
                let kept_cpu = cpu_frame(&mut arms, &mut t);
                feed(&mut t, b"\x1b[?25h");
                // The frame shown before the switch, with the cursor, its
                // glow, the guide and a trail.
                let (row, col) = t.cursor();
                arms.trail = Some(TrailLook::Ghost(Trail {
                    from: (
                        Row::from_index(row.index().saturating_sub(2)),
                        Col::at_index(0),
                    ),
                    to: (row, col),
                    progress: 0.4,
                    origin_dy: 0.0,
                    lift: Px::of(0),
                    strength: baud_render::Strength::Full,
                }));
                let (shown_a, shown_b) = arms.draw(&mut t);
                assert_eq!(
                    differing(&shown_a, &shown_b),
                    0,
                    "{at}: the frames shown differ"
                );
                assert!(
                    differing(&shown_a, &kept_a) > 0,
                    "{at}: the frame shown draws the cursor"
                );
                feed(&mut t, switch);
                if toward == Toward::Alternate {
                    feed(&mut t, program);
                }
                match keeper {
                    "instances" => arms.keep_instances(&t),
                    _ => arms.keep_cells(&t),
                }
                // The new screen under the slide's faint veil, as a driver
                // draws it while it slides, with no trail.
                arms.trail = None;
                arms.fade = Some(Transition::faint(0.0));
                let (live_a, live_b) = arms.draw(&mut t);
                assert_eq!(
                    differing(&live_a, &live_b),
                    0,
                    "{at}: the new frames differ"
                );
                let live_cpu = cpu_frame(&mut arms, &mut t);
                let (fw, fh) = arms.renderer.whole_size();
                let (w, h) = (
                    usize::try_from(fw.value() + margin.0).unwrap(),
                    usize::try_from(fh.value() + margin.1).unwrap(),
                );
                let rows = usize::try_from(fh.value()).unwrap();
                let top = (h - rows) / 2;
                for p in [0.0, 0.3, 0.75] {
                    let at = format!("{at} at {p}");
                    let slide = ScreenSlide {
                        progress: p,
                        toward,
                    };
                    arms.gpu.set_slide(Some(slide));
                    let (a, b) = arms.draw(&mut t);
                    assert_eq!(differing(&a, &b), 0, "{at}: the GPU arms disagree");
                    let o = slide_offsets(slide, fh);
                    let mut want = live_a.clone();
                    let mut mid = vec![0u32; w * rows];
                    assert!(compose_slide(
                        &band(&live_a, w, top, rows),
                        &band(&kept_a, w, top, rows),
                        &mut mid,
                        Px::of(u32::try_from(w).unwrap()),
                        fh,
                        o,
                    ));
                    want[top * w..(top + rows) * w].copy_from_slice(&mid);
                    assert_eq!(differing(&a, &want), 0, "{at}: not the slide of its frames");
                    assert!(
                        differing(&a, &live_a) > 0,
                        "{at}: the slide drew the new frame"
                    );
                    // At the start the new screen is wholly below or above.
                    assert_eq!(
                        differing(&a, &kept_a) > 0,
                        p > 0.0,
                        "{at}: the kept frame, against the start"
                    );
                    if margin == (0, 0) {
                        let fwu = usize::try_from(fw.value()).unwrap();
                        let mut cpu = vec![0u32; fwu * rows];
                        assert!(compose_slide(&live_cpu, &kept_cpu, &mut cpu, fw, fh, o));
                        assert_eq!(differing(&cpu, &a), 0, "{at}: the CPU arm's slide differs");
                    }
                }
                arms.gpu.set_slide(None);
                arms.fade = None;
            }
        }
    }
}

/// **A switch while the screen slides keeps the screen unslid**, as sfte's
/// outgoing pass draws its parked screen at rest: into the alternate
/// screen, a frame of the slide drawn, and then out of it before the slide
/// is over. The screen kept is the alternate's, whole and in place with its
/// cursor hidden, the Sixel on its cursor's row that leaving deleted
/// included -- the CPU arm's `?25l` frame of it, which a frame of the
/// slide at its start shows on either GPU path's keeping.
#[test]
fn a_switch_while_the_screen_slides_keeps_it_unslid() {
    let font = |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let regular = font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&regular).unwrap()];
    for keeper in ["instances", "cells"] {
        let renderer = Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap();
        arms!(renderer => arms, t);
        arms.reads = Reads::Copy;
        let keep = |arms: &mut Arms<'_, '_>, t: &Term<'_>| match keeper {
            "instances" => arms.keep_instances(t),
            _ => arms.keep_cells(t),
        };
        feed(
            &mut t,
            b"user@host:~/src$ ls\r\nbaud-app  baud-core  baud-render  docs\r\n$ vim main.rs\r\n",
        );
        let _ = arms.draw(&mut t);
        // The program's screen, a Sixel beside the cursor's row: leaving
        // deletes it before the switch is seen.
        let mut program =
            b"\x1b[?1049h\x1b[33m  1 \x1b[0mfn main() { println!(\"hello\"); }\r\n\x1b[34m~\x1b[0m\x1b[1;34H"
                .to_vec();
        program.extend_from_slice(b"\x1bP0;1;0q\"1;1;24;36#1;2;90;10;10");
        program.extend_from_slice("#1!24~-".repeat(6).as_bytes());
        program.extend_from_slice(b"\x1b\\\x1b[1;5H");
        feed(&mut t, &program);
        keep(&mut arms, &t);
        // The alternate screen with its cursor hidden, on the CPU arm.
        feed(&mut t, b"\x1b[?25l");
        let want = cpu_frame(&mut arms, &mut t);
        let red = |frame: &[u32]| {
            frame
                .iter()
                .filter(|&&p| {
                    let [_, r, g, b] = p.to_be_bytes();
                    r > 200 && g < 60 && b < 60
                })
                .count()
        };
        assert!(red(&want) > 0, "{keeper}: the Sixel is drawn");
        feed(&mut t, b"\x1b[?25h");
        // A frame of the slide in, drawn with the cursor: the last frame
        // shown before the switch out.
        arms.gpu.set_slide(Some(ScreenSlide {
            progress: 0.3,
            toward: Toward::Alternate,
        }));
        let (slid, _) = arms.draw(&mut t);
        assert!(differing(&slid, &want) > 0, "{keeper}: the frame slid");
        feed(&mut t, b"\x1b[?1049l");
        keep(&mut arms, &t);
        arms.gpu.set_slide(Some(ScreenSlide {
            progress: 0.0,
            toward: Toward::Primary,
        }));
        let (a, b) = arms.draw(&mut t);
        assert_eq!(differing(&a, &b), 0, "{keeper}: the GPU arms disagree");
        assert_eq!(
            differing(&a, &want),
            0,
            "{keeper}: the screen kept is not the alternate's, in place, with no cursor"
        );
        arms.gpu.set_slide(None);
    }
}

/// **No surface keeps a slid frame once the slide is over**: three targets
/// rotated as the presenter's surfaces are, on both GPU paths, the frame
/// before the switch drawn, the old screen kept, three frames of the slide, then the
/// screen still for a turn of the surfaces and one more -- each frame after
/// the slide read back against the new screen drawn whole. A surface whose
/// last frame was slid must be drawn whole when it comes round, though
/// nothing on the screen changed since.
#[test]
fn no_surface_keeps_a_slid_frame() {
    let files = partial_faces();
    let faces = files.each_ref().map(|f| Face::parse(f).unwrap());
    for path in ["instances", "cells"] {
        let renderer = partial_renderer(&faces);
        arms!(renderer => arms, t);
        arms.reads = Reads::Copy;
        let size = arms.renderer.whole_size();
        let n = usize::try_from(size.0.value() * size.1.value()).unwrap();
        let whole = arms.gpu.offscreen(size.0, size.1).expect("target");
        let mut rot = Rotation::new(&arms.gpu, size);
        feed(&mut t, b"$ ls\r\nbaud-app  baud-core  docs\r\n$ vim\r\n");
        // One frame into the rotation, drawn as `path` draws it.
        let frame = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, rot: &mut Rotation| -> Vec<u32> {
            let missing = rot.ages.missing(rot.next);
            let f = match path {
                "instances" => {
                    arms.renderer.set_images(t, arms.back);
                    arms.renderer.convert(t, arms.back, Damage::Incremental);
                    arms.gpu
                        .draw_since(&mut arms.renderer, rot.target(), Done::Wait, missing)
                }
                _ => arms.with_cells(t, |gpu, renderer, input| {
                    gpu.draw_cells_since(renderer, input, rot.target(), Done::Wait, missing)
                }),
            };
            let Some(Drawn::Frame(f)) = f else {
                panic!("{path}: a frame");
            };
            let mut p = vec![0u32; n];
            arms.gpu.read(rot.target(), &mut p).expect("read");
            rot.drew(&f, Counted::No);
            p
        };
        for _ in 0..SURFACES {
            let _ = frame(&mut arms, &mut t, &mut rot);
        }
        feed(
            &mut t,
            b"\x1b[?1049h\x1b[H\x1b[2J  1 fn main() {}\r\n~\r\n~",
        );
        // The old screen kept as the path keeps it once the switch is read.
        match path {
            "instances" => arms.keep_instances(&t),
            _ => arms.keep_cells(&t),
        }
        // The new screen, whole and unslid: the slide is set but not begun.
        arms.renderer.convert(&t, arms.back, Damage::Full);
        let Some(Drawn::Frame(_)) = arms.gpu.draw(&mut arms.renderer, &whole, Done::Wait) else {
            panic!("draw");
        };
        let mut settled = vec![0u32; n];
        arms.gpu.read(&whole, &mut settled).expect("read");
        for p in [0.2, 0.5, 0.8] {
            arms.gpu.set_slide(Some(ScreenSlide {
                progress: p,
                toward: Toward::Alternate,
            }));
            let slid = frame(&mut arms, &mut t, &mut rot);
            assert!(differing(&slid, &settled) > 0, "{path} at {p}: slid");
        }
        arms.gpu.set_slide(None);
        for k in 0..=SURFACES {
            let p = frame(&mut arms, &mut t, &mut rot);
            assert_eq!(
                differing(&p, &settled),
                0,
                "{path}: frame {k} after the slide shows a slid frame"
            );
        }
    }
}

/// **The most a byte's exact value can lie from a rounding tie and the CPU
/// arm's `Blend::over_unit` still round it across**, in levels: its `f32`
/// `Cs a + cb (1 - a)` rounds four times below 1, at most 3.5 half-ulps of
/// 2^-24 there, and `* 255` once more, half of 2^-16: 3.5 * 2^-25 * 255 +
/// 2^-17 = 3.42e-5. The GPU's store rounds less (`src`, `1 - a` and one
/// fused multiply-add, then the exact product), so no pixel the arms round
/// differently at one light lies farther from a tie.
const TIE_LEVELS: f64 = 3.5e-5;

/// **Pixels of a plasma frame the arms may differ on off a tie**: where the
/// shader's light is some ulps from the CPU's, in the filaments, or an ion
/// lies over a band, and the byte is near a tie by chance: at most 2 on
/// 9,350 frames, 275 ages of each half at each of 17 cell tops.
const TIE_STRAYS: usize = 2;

/// **The differing pixels of a splash plasma frame, as the two kinds its
/// rounding gives** (strays, on a tie): a pixel is on a tie when, from the
/// band's ink over the bare frame's pixel at [`plasma::light`], every channel
/// the arms differ on has its exact value within [`TIE_LEVELS`] of a half. A
/// level band's row has one light over one ground, so one tie takes the
/// whole row of one ink at once, and no count of pixels bounds it; a real
/// difference is not at a tie, and is a stray however few rows it takes.
fn off_the_ties(
    arms: &mut Arms<'_, '_>,
    cpu: &[u32],
    gpu: &[u32],
    plain: &[u32],
    (age, colours): (f32, baud_render::splash::Colours),
) -> (usize, usize) {
    let frame = arms.renderer.whole_size();
    let (w, h) = (frame.0.value() as f32, frame.1.value() as f32);
    let scale = baud_render::splash::scale((w, h));
    let at = baud_render::splash::origin((w, h), scale);
    let vfx = baud_render::splash::vfx(at, scale, age);
    let fw = usize::try_from(frame.0.value()).unwrap();
    let mut records = Vec::new();
    vfx.instances(frame, colours.amber, &mut records);
    baud_render::splash::paint(&mut records, at, scale, colours);
    let glow = baud_render::splash::glow(age);
    let (mut strays, mut ties) = (0, 0);
    for (i, (&c, &g)) in cpu.iter().zip(gpu).enumerate() {
        if c == g {
            continue;
        }
        let (x, y) = (
            i32::try_from(i % fw).unwrap(),
            i32::try_from(i / fw).unwrap(),
        );
        let band = records.iter().find(|r| {
            let (rx, ry) = (i32::from(r.x.0), i32::from(r.y.0));
            r.kind == Kind::Band
                && (rx..rx + i32::from(r.w.0)).contains(&x)
                && (ry..ry + i32::from(r.h.0)).contains(&y)
        });
        let tie = band.is_some_and(|r| {
            let mut p = at;
            p.x = x as f32 + 0.5;
            p.y = y as f32 + 0.5;
            let a = f64::from(baud_render::plasma::light(&vfx, p))
                * f64::from(Chan::from_unit(glow).get())
                / 255.0;
            [16u32, 8, 0].iter().all(|&k| {
                let ch = |v: u32| (v >> k) & 0xFF;
                if ch(c) == ch(g) {
                    return true;
                }
                let v = f64::from(ch(r.color.0)) * a + f64::from(ch(plain[i])) * (1.0 - a);
                (v - v.floor() - 0.5).abs() <= TIE_LEVELS
            })
        });
        match tie {
            true => ties += 1,
            false => strays += 1,
        }
    }
    (strays, ties)
}

/// **The launch splash draws alike on every arm** (note AJ): the lockup
/// held, the plasma at three ages as the logo comes out, three as the
/// second plasma carries it out from the frame it lights, one as what is
/// left of it fades, and its end, and the fade
/// at half, over a prompt and a line of text under the logo, on the dark
/// and the light
/// half. The cell arm draws what the instance arm draws, pixel for pixel;
/// the CPU arm, which the Linux driver draws with, the same, but for the
/// plasma's own rounding against the GPU's -- a step at most, on a tie
/// ([`off_the_ties`]), which takes a band's row of one ink at once, 76
/// pixels of word ink where the plasma lights on the light half, and off
/// the ties at most [`TIE_STRAYS`] pixels a frame -- as
/// [`effect_scenes`] holds each GPU arm to the effect's reference. The
/// controls: the held logo changes the frame and its block is the half's
/// amber, exactly, at the block's centre; each plasma lights pixels the
/// held logo does not; the first frame differs from the held logo; at
/// its end the frame is the plain one. Both plasmas at full strength: no
/// frame carries a cap.
#[test]
fn the_splash_draws_alike_on_every_arm() {
    let font = |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let regular = font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf");
    let bold = font("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf");
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    let roles = [FaceRole::Fallback, FaceRole::Style(FaceStyle::Bold)];
    let renderer = Renderer::new(
        Stack::with_roles(&faces, &roles).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    arms!(renderer => arms, t);
    feed(
        &mut t,
        b"\x1b[1;34m~/code\x1b[0m on \x1b[35mmain\x1b[0m\r\n\x1b[5;30Hunder the logo, text\x1b[2;1H$ ",
    );
    let (out, gone) = (baud_render::splash::OUT, baud_render::splash::GONE);
    let swept = baud_render::splash::LEFT;
    for (half, theme, colours) in [
        ("dark", Theme::dark(), baud_render::splash::DARK),
        ("light", Theme::light(), baud_render::splash::light()),
    ] {
        let amber = colours.amber;
        arms.renderer.set_theme(theme);
        arms.splash = None;
        let (plain, _) = arms.draw(&mut t);
        let mut held = Vec::new();
        for phase in [
            SplashPhase::Hold,
            SplashPhase::Plasma(0.0),
            SplashPhase::Plasma(0.03),
            SplashPhase::Plasma(0.07),
            SplashPhase::Plasma(out),
            SplashPhase::Plasma(out + 0.05),
            SplashPhase::Plasma(out + 0.1),
            SplashPhase::Plasma((swept + gone) / 2.0),
            SplashPhase::Fade(0.5),
            SplashPhase::Plasma(gone),
        ] {
            let at = format!("{half}, {phase:?}");
            arms.splash = Some(SplashLook { phase });
            // Both plasmas at full strength: no frame waits on a cap.
            let (a, b) = arms.draw(&mut t);
            assert_eq!(
                arms.renderer.flash_held(),
                baud_render::Held::Full,
                "{at}: no repeat pass"
            );
            assert_eq!(differing(&a, &b), 0, "{at}: the GPU arms disagree");
            let cpu = cpu_frame(&mut arms, &mut t);
            let (n, most) = channel_diff(&cpu, &a, |_| true);
            match phase {
                SplashPhase::Plasma(age) if age < gone => {
                    let (strays, ties) = off_the_ties(&mut arms, &cpu, &a, &plain, (age, colours));
                    assert!(
                        most <= 1 && strays <= TIE_STRAYS,
                        "{at}: the CPU arm differs on {n}, by {most}: {ties} on a tie, {strays} off one"
                    );
                }
                _ => assert_eq!((n, most), (0, 0), "{at}: the CPU arm differs"),
            }
            match phase {
                SplashPhase::Hold => {
                    assert!(differing(&a, &plain) > 500, "{at}: the control, drawn");
                    let records = arms.renderer.splash();
                    let block = records.last().expect("the block's record");
                    assert_eq!(block.kind, Kind::Solid);
                    let fw = usize::try_from(arms.renderer.frame_size().0.value()).unwrap();
                    let (cx, cy) = (
                        usize::try_from(i32::from(block.x.0) + i32::from(block.w.0) / 2).unwrap(),
                        usize::try_from(i32::from(block.y.0) + i32::from(block.h.0) / 2).unwrap(),
                    );
                    assert_eq!(
                        a[cy * fw + cx],
                        premultiply(amber).0,
                        "{at}: the block is the half's amber"
                    );
                    held = a;
                }
                SplashPhase::Plasma(age) if age >= gone => {
                    assert_eq!(differing(&a, &plain), 0, "{at}: gone");
                    assert!(arms.renderer.splash().is_empty(), "{at}: no record left");
                }
                SplashPhase::Plasma(0.0) => {
                    assert!(differing(&a, &held) > 500, "{at}: not yet whole");
                    assert!(differing(&a, &plain) > 500, "{at}: on screen at once");
                }
                SplashPhase::Plasma(age) if age > 0.0 && age < out + 0.1 => {
                    let new = a
                        .iter()
                        .zip(&held)
                        .zip(&plain)
                        .filter(|((x, h), p)| x != h && x != p)
                        .count();
                    assert!(new > 100, "{at}: the plasma lit only {new} pixels");
                }
                _ => {}
            }
        }
    }
    arms.splash = None;
}

/// **The shell is drawn in front of the splash on every arm**
/// (`Renderer::splash_yields_to`): with four lines of output reaching into
/// the logo's box, at the held logo, three ages of the plasma and the
/// fade, every cell the output inked is drawn by the instance arm and the
/// cell arm, pixel for pixel, as each draws it with no splash -- the cut
/// glyph records sampled where the CPU path draws them -- and the two GPU
/// arms agree on the whole frame. The controls: the logo is drawn, the
/// frame differing from the plain one outside the shell's cells; and the
/// output reaches the logo's box.
#[test]
fn the_shell_is_drawn_in_front_of_the_splash_on_every_arm() {
    let font = |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let regular = font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf");
    let bold = font("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf");
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    let roles = [FaceRole::Fallback, FaceRole::Style(FaceStyle::Bold)];
    let renderer = Renderer::new(
        Stack::with_roles(&faces, &roles).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    arms!(renderer => arms, t);
    for i in 0..4 {
        feed(
            &mut t,
            format!(
                "\x1b[{};1Hline {i}: the shell's output, drawn in front of the logo it meets",
                i + 4
            )
            .as_bytes(),
        );
    }
    feed(&mut t, b"\x1b[8;1H\x1b[1;34m~/code\x1b[0m $ ");
    let m = arms.renderer.metrics();
    let n = |p: Px| usize::try_from(p.value()).unwrap();
    let (cw, ch, fw) = (n(m.cell_w), n(m.cell_h), n(arms.renderer.frame_size().0));
    let view = t.view(Rows::ZERO);
    let mut inked = Vec::new();
    for y in 0..view.rows().index() {
        for (x, c) in view.row(Row::from_index(y)).iter().enumerate() {
            if !c.content.is_empty() && c.content.as_codepoint().is_none_or(|p| p.value() != 0x20) {
                inked.push((y, x));
            }
        }
    }
    let cell = |px: &[u32], (y, x): (usize, usize)| -> Vec<u32> {
        (0..ch)
            .flat_map(|dy| px[(y * ch + dy) * fw + x * cw..][..cw].to_vec())
            .collect()
    };
    let shell = |i: usize| inked.contains(&((i / fw) / ch, (i % fw) / cw));
    arms.splash = None;
    let (plain, _) = arms.draw(&mut t);
    let out = baud_render::splash::OUT;
    let mut reached = false;
    for phase in [
        SplashPhase::Hold,
        SplashPhase::Plasma(0.03),
        SplashPhase::Plasma(out),
        SplashPhase::Plasma(out + 0.1),
        SplashPhase::Fade(0.5),
    ] {
        arms.splash = Some(SplashLook { phase });
        let (a, b) = arms.draw(&mut t);
        assert_eq!(differing(&a, &b), 0, "{phase:?}: the GPU arms disagree");
        for &c in &inked {
            assert!(
                cell(&a, c) == cell(&plain, c),
                "{phase:?}: the instance arm draws cell {c:?} under the logo"
            );
            assert!(
                cell(&b, c) == cell(&plain, c),
                "{phase:?}: the cell arm draws cell {c:?} under the logo"
            );
        }
        let logo = (0..fw * ch * usize::from(WS.ws_row))
            .filter(|&i| !shell(i) && a[i] != plain[i])
            .count();
        assert!(
            logo > 100,
            "{phase:?}: the control, the logo drawn ({logo})"
        );
        reached |= arms
            .renderer
            .splash()
            .iter()
            .chain(arms.renderer.trail())
            .any(|r| {
                let right = usize::try_from(i32::from(r.x.0) + i32::from(r.w.0)).unwrap_or(0);
                right > 0
                    && inked.iter().any(|&(y, x)| {
                        let (top, bottom) = (i32::from(r.y.0), i32::from(r.y.0) + i32::from(r.h.0));
                        let row = i32::try_from(y * ch).unwrap();
                        x * cw < right && row < bottom && row + i32::try_from(ch).unwrap() > top
                    })
            });
    }
    assert!(
        reached,
        "the control: the output reaches the logo's rows and columns"
    );
    arms.splash = None;
}

/// **A smooth scroll's slide keeps the shell in front of the splash on the
/// cell arm** (`Renderer::splash_yields_to`'s `lift`): the grid and its
/// leaving row are drawn lowered by the pan and the logo is not, so each
/// row's hole reaches the pan further down and the leaving row's is on
/// the window's top rows. With alternate long and short lines filling the
/// window and the row above it, at the held logo and two ages of the
/// plasma, a pan of half a cell and of a quarter draws every pixel the
/// slide inks as the same slide draws it with no splash. The controls:
/// the slide moves the frame, and the logo is drawn.
#[test]
fn a_slide_draws_none_of_the_shells_ink_under_the_logo() {
    let font = |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let regular = font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf");
    let bold = font("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf");
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    let roles = [FaceRole::Fallback, FaceRole::Style(FaceStyle::Bold)];
    let renderer = Renderer::new(
        Stack::with_roles(&faces, &roles).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    arms!(renderer => arms, t);
    feed(&mut t, b"\x1b[?25l\x1b[2J\x1b[H");
    for i in 0..14u32 {
        let line = match i % 2 {
            0 => format!(
                "{i:02} {}\r\n",
                "abcdefghijklmnopqrstuvwxyzWgjpqy".repeat(3)
            ),
            _ => format!("{i:02}\r\n"),
        };
        feed(&mut t, line.as_bytes());
    }
    let cell_h = arms.renderer.metrics().cell_h;
    // The leaving row's glyphs, walked into the map as the app walks them
    // over the window one line up.
    arms.renderer
        .sync_styles(&t, arms.styles_mem, &mut arms.stamp);
    arms.map.ensure(
        &t,
        t.view(Rows::of_count(1)),
        arms.styles_mem,
        &mut arms.renderer,
        &mut arms.candidates,
    );
    arms.back = Rows::ZERO;
    arms.slide = None;
    arms.splash = None;
    let (_, still) = arms.draw(&mut t);
    for pan in [cell_h.value() / 2, cell_h.value() / 4] {
        arms.slide = Some(Px::of(pan));
        arms.splash = None;
        let (_, plain) = arms.draw(&mut t);
        let bg = plain[plain.len() - 1];
        assert!(
            differing(&still, &plain) > 100,
            "the control: a pan of {pan} px moves the frame"
        );
        for phase in [
            SplashPhase::Hold,
            SplashPhase::Plasma(0.2),
            SplashPhase::Plasma(0.3),
        ] {
            arms.splash = Some(SplashLook { phase });
            let (_, b) = arms.draw(&mut t);
            let under = plain
                .iter()
                .zip(&b)
                .filter(|(p, q)| **p != bg && p != q)
                .count();
            assert_eq!(
                under, 0,
                "pan {pan} px, {phase:?}: ink pixels drawn under the logo"
            );
            assert!(
                differing(&plain, &b) > 100,
                "pan {pan} px, {phase:?}: the control, the logo drawn"
            );
        }
    }
    arms.splash = None;
    arms.slide = None;
}

/// **iTerm2's fireworks draw alike on every arm** (`baud_render::fireworks`):
/// a show at five ages from its first frame through its fade, and the still
/// form Reduce Motion draws, over a prompt and a line of text, the cursor's
/// cell the hole's centre, on the dark and the light theme. The cell arm
/// draws what the instance arm draws, pixel for pixel; the CPU arm, which
/// the Linux driver draws with, the same but for the GPU's own rounding --
/// a step, on at most ten pixels (one was measured), where the GPU's turn
/// and square root leave a coverage a hair across a byte from the CPU's.
/// Letterboxed by a
/// margin, both GPU arms draw the frame they draw at the origin, moved:
/// the hole is cut where the frame's cursor is, not the target's. The
/// controls: the first frame is the plain one, every star born in the
/// hole; every frame after it differs from the plain frame by more than a
/// hundred pixels; the end is the plain frame, with no record left.
#[test]
fn the_fireworks_draw_alike_on_every_arm() {
    use baud_render::fireworks::{BAUD, FADE, Form, HOLD, ORIGINS, Show};
    let font = |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let regular = font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&regular).unwrap()];
    let renderer = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    arms!(renderer => arms, t);
    feed(
        &mut t,
        b"\x1b[1;34m~/code\x1b[0m on \x1b[35mmain\x1b[0m\r\n\x1b[3;10HThe quick brown fox jumps over the lazy dog, 0123456789\x1b[6;60H",
    );
    let m = arms.renderer.metrics();
    let f = |v: u32| f32::from(u16::try_from(v).unwrap());
    let at = Pt {
        x: 59.5 * f(m.cell_w.value()),
        y: 5.5 * f(m.cell_h.value()),
    };
    let show = |age: f32, form: Form| Show {
        age,
        hold: HOLD,
        origins: [at; ORIGINS],
        hole: at,
        scale: 1.0,
        seed: Seed(0x51DE),
        look: BAUD,
        form,
    };
    let end = HOLD + FADE;
    let mut worst = (0, 0);
    for (half, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
        arms.renderer.set_theme(theme);
        arms.fireworks = Vec::new();
        let (plain, _) = arms.draw(&mut t);
        for (age, form) in [
            (0.0, Form::Moving),
            (0.1, Form::Moving),
            (0.4, Form::Moving),
            (0.9, Form::Moving),
            (HOLD + FADE / 2.0, Form::Moving),
            (0.3, Form::Still),
            (end, Form::Moving),
        ] {
            let what = format!("{half}, {form:?} at {age}");
            arms.fireworks = vec![show(age, form)];
            let (a, b) = arms.draw(&mut t);
            assert_eq!(differing(&a, &b), 0, "{what}: the GPU arms disagree");
            let cpu = cpu_frame(&mut arms, &mut t);
            let (n, most) = channel_diff(&cpu, &a, |_| true);
            eprintln!(
                "{what}: the CPU arm differs on {n}, by {most}; {} pixels drawn",
                differing(&a, &plain)
            );
            assert!(
                most <= 1 && n <= 10,
                "{what}: the CPU arm differs on {n}, by {most}"
            );
            worst = (worst.0.max(n), worst.1.max(most));
            if age >= end {
                assert_eq!(differing(&a, &plain), 0, "{what}: gone");
                assert!(arms.renderer.trail().is_empty(), "{what}: no record left");
            } else if age == 0.0 {
                // Born at the cursor's centre, every star is in the hole.
                assert_eq!(differing(&a, &plain), 0, "{what}: all in the hole");
            } else {
                assert!(differing(&a, &plain) > 100, "{what}: the control, drawn");
            }
        }
        // Letterboxed: the same frame, moved by the margin.
        arms.fireworks = vec![show(0.4, Form::Moving)];
        let (at_origin, _) = arms.draw(&mut t);
        arms.margin = (14, 10);
        let (a, b) = arms.draw(&mut t);
        assert_eq!(
            differing(&a, &b),
            0,
            "{half}, letterboxed: the GPU arms disagree"
        );
        let (tw, _) = arms.target_size();
        let (fw, fh) = arms.renderer.whole_size();
        let (tw, fw, fh) = (
            usize::try_from(tw.value()).unwrap(),
            usize::try_from(fw.value()).unwrap(),
            usize::try_from(fh.value()).unwrap(),
        );
        let mut moved = 0;
        for y in 0..fh {
            for x in 0..fw {
                moved += usize::from(a[(y + 5) * tw + x + 7] != at_origin[y * fw + x]);
            }
        }
        assert_eq!(
            moved, 0,
            "{half}, letterboxed: the frame is not the one at the origin"
        );
        arms.margin = (0, 0);
    }
    eprintln!("the CPU arm's worst: {} pixels, by {}", worst.0, worst.1);
    arms.fireworks = Vec::new();
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
    arms!(renderer => arms, t);
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
        // is its shape at half alpha; the glow, the fire and the ufo are
        // solids over the cell. The arms must agree to the bit, the CPU arm,
        // which the Linux driver draws with, included, and each frame must be
        // neither phase, or a fade or glow lost on every arm passes here.
        for (p, what) in blink_phases() {
            arms.renderer.set_cursor_blink(p);
            let (a, b) = arms.draw(&mut t);
            assert_eq!(
                differing(&a, &b),
                0,
                "the arms disagree on a {name} cursor {what}"
            );
            let cpu = cpu_frame(&mut arms, &mut t);
            let d = channel_diff(&cpu, &a, |_| true);
            assert_eq!(
                d.0, 0,
                "the CPU arm differs on a {name} cursor {what}, by up to {}",
                d.1
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
            let cpu = cpu_frame(&mut arms, &mut t);
            let d = channel_diff(&cpu, &a, |_| true);
            assert_eq!(
                d.0, 0,
                "the CPU arm differs on a {name} cursor {what} over a record, by up to {}",
                d.1
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
    arms.highlight = Some(Highlight::new(
        (Row::from_index(1), Col::at_index(58)),
        (Row::from_index(1), Col::at_index(70)),
        Region::Stream,
    ));
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
    // **A faint off phase**, as Dim Flashing Lights draws it: each rate's
    // text half way to its page -- the glyphs, an underline in its own
    // `SGR 58` colour, a strike and a framed run -- and under a block
    // cursor on a faint cell. The arms agree, and faint is neither the
    // shown frame nor the hidden one.
    let faint_words =
        b"\x1b[3;60H\x1b[5;4;9;51;31;58;5;3mSLOW\x1b[0m \x1b[6;4;9;32;58;5;5mFAST\x1b[0m";
    let mut shown_faint = None;
    for (phase, why) in [
        (BlinkPhases::SHOWN, "the shown phase"),
        (
            BlinkPhases {
                slow: Blink::Faint,
                rapid: Blink::Faint,
            },
            "both rates faint",
        ),
        (
            BlinkPhases {
                slow: Blink::Hidden,
                rapid: Blink::Hidden,
            },
            "both rates hidden",
        ),
    ] {
        arms.renderer.set_blink(phase);
        feed(&mut t, faint_words);
        let (a, b) = arms.draw(&mut t);
        assert_eq!(differing(&a, &b), 0, "the arms disagree on {why}");
        feed(&mut t, b"\x1b[3;60H\x1b[K");
        match shown_faint {
            None => shown_faint = Some((a, None)),
            Some((ref on, None)) => {
                assert_ne!(
                    differing(on, &a),
                    0,
                    "the faint phase drew the text in full"
                );
                shown_faint = Some((on.clone(), Some(a)));
            }
            Some((_, Some(ref faint))) => {
                assert_ne!(differing(faint, &a), 0, "the faint phase hid the text");
            }
        }
    }
    arms.renderer.set_blink(BlinkPhases {
        slow: Blink::Faint,
        rapid: Blink::Shown,
    });
    feed(&mut t, faint_words);
    feed(&mut t, b"\x1b[?25h\x1b[2 q\x1b[3;61H");
    let (a, b) = arms.draw(&mut t);
    assert_eq!(
        differing(&a, &b),
        0,
        "the arms disagree on a block cursor over faint text"
    );
    feed(&mut t, b"\x1b[?25l\x1b[3;60H\x1b[K");
    arms.renderer.set_blink(BlinkPhases::SHOWN);
    feed(&mut t, b"\x1b[?25h");
    // **iTerm2's cursor guide** over the cursor's row, under its text and
    // its cursor, a block's cell left out, for each cursor shape. The arms
    // agree, and each draws it: the frame differs from the one without.
    feed(&mut t, b"\x1b[3;60Hguide \x1b[44mblue\x1b[0m\x1b[3;62H");
    let look = Some(baud_render::GuideLook {
        colour: baud_render::CURSOR_GUIDE,
        line: Px::of(2),
    });
    for (shape, name) in [
        (b"\x1b[2 q", "block"),
        (b"\x1b[4 q", "underline"),
        (b"\x1b[6 q", "bar"),
    ] {
        feed(&mut t, shape);
        arms.guide = None;
        let (bare_a, bare_b) = arms.draw(&mut t);
        arms.guide = look;
        let (a, b) = arms.draw(&mut t);
        assert_eq!(
            differing(&a, &b),
            0,
            "the arms disagree on the guide, {name}"
        );
        assert_ne!(
            differing(&a, &bare_a),
            0,
            "the instance arm drew no guide, {name}"
        );
        assert_ne!(
            differing(&b, &bare_b),
            0,
            "the cell arm drew no guide, {name}"
        );
    }
    // While a smooth scroll pans the cell arm's grid, the guide pans with
    // the cursor's row: the pixel rows it changes are the resting ones
    // lowered by the pan.
    let width = usize::try_from(arms.renderer.whole_size().0.value() + arms.margin.0).unwrap();
    let changed = |a: &[u32], b: &[u32]| -> Vec<usize> {
        (0..a.len() / width)
            .filter(|&y| a[y * width..(y + 1) * width] != b[y * width..(y + 1) * width])
            .collect()
    };
    arms.guide = None;
    let (_, rest_bare) = arms.draw(&mut t);
    arms.guide = look;
    let (_, rest) = arms.draw(&mut t);
    arms.slide = Some(Px::of(5));
    arms.guide = None;
    let (_, slid_bare) = arms.draw(&mut t);
    arms.guide = look;
    let (_, slid) = arms.draw(&mut t);
    arms.slide = None;
    let resting = changed(&rest, &rest_bare);
    assert!(!resting.is_empty(), "the control: the guide draws at rest");
    assert_eq!(
        changed(&slid, &slid_bare),
        resting.iter().map(|y| y + 5).collect::<Vec<_>>(),
        "the guide stays on the cursor's row through a slide"
    );
    arms.guide = None;
    // **iTerm2's badge** in the top right, its text over it: the arms
    // agree, and each draws it.
    feed(&mut t, b"\x1b[2;40Hthe text that runs under the badge");
    let (bare_a, bare_b) = arms.draw(&mut t);
    let unlined = baud_render::BadgeLook {
        text: "prod-db-01\nmain".into(),
        colour: baud_render::BADGE,
        outline: baud_render::Argb(0),
        drawn_at: baud_render::DRAWN_AT,
        fraction: baud_render::BOX,
        top: Px::of(30),
        right: Px::of(10),
        scale: 1.0,
        style: FaceStyle::Bold,
    };
    arms.badge = Some(unlined.clone());
    let (plain, _) = arms.draw(&mut t);
    // Its outline in the page's colour, drawn over each glyph on every arm:
    // the GPU arms agree, the CPU arm, which the Linux driver draws with,
    // draws the same, and the outline moves pixels the badge alone does not.
    arms.badge = Some(baud_render::BadgeLook {
        outline: arms
            .renderer
            .background_of(&t)
            .with_alpha(baud_render::DRAWN_AT),
        ..unlined
    });
    let (a, b) = arms.draw(&mut t);
    assert_eq!(differing(&a, &b), 0, "the arms disagree on the badge");
    assert_ne!(differing(&a, &bare_a), 0, "the instance arm drew no badge");
    assert_ne!(differing(&b, &bare_b), 0, "the cell arm drew no badge");
    assert_ne!(differing(&a, &plain), 0, "the outline drew nothing");
    let cpu = cpu_frame(&mut arms, &mut t);
    assert_eq!(
        channel_diff(&cpu, &a, |_| true),
        (0, 0),
        "the CPU arm draws the outlined badge otherwise"
    );
    // A colour glyph in it, drawn inside the badge's alpha on every arm:
    // at half it differs from the glyph drawn whole.
    let outlined = arms.badge.clone();
    let mut emoji = Vec::new();
    for drawn_at in [baud_render::DRAWN_AT, baud_domain::Chan::from_u8(0xFF)] {
        arms.badge = outlined.clone().map(|l| baud_render::BadgeLook {
            text: "\u{231A} main".into(),
            drawn_at,
            ..l
        });
        let (a, b) = arms.draw(&mut t);
        assert_eq!(differing(&a, &b), 0, "the arms disagree on a colour glyph");
        let cpu = cpu_frame(&mut arms, &mut t);
        assert_eq!(
            channel_diff(&cpu, &a, |_| true),
            (0, 0),
            "the CPU arm draws the colour glyph otherwise"
        );
        emoji.push(a);
    }
    assert_ne!(
        differing(&emoji[0], &emoji[1]),
        0,
        "drawn at half and whole alike"
    );
    arms.badge = outlined;
    // A block cursor inside the badge's rectangle is drawn over it, on
    // both arms: its cell's pixels are the frame's without the badge.
    let rect =
        arms.renderer
            .badge()
            .iter()
            .fold([i32::MAX, i32::MAX, i32::MIN, i32::MIN], |r, i| {
                let (x, y) = (i32::from(i.x.0), i32::from(i.y.0));
                [
                    r[0].min(x),
                    r[1].min(y),
                    r[2].max(x + i32::from(i.w.0)),
                    r[3].max(y + i32::from(i.h.0)),
                ]
            });
    let m = arms.renderer.metrics();
    let (cw, ch) = (
        i32::try_from(m.cell_w.value()).unwrap(),
        i32::try_from(m.cell_h.value()).unwrap(),
    );
    let (row, col) = ((rect[1] + rect[3]) / 2 / ch, (rect[0] + rect[2]) / 2 / cw);
    feed(
        &mut t,
        format!("\x1b[2 q\x1b[?25h\x1b[{};{}H", row + 1, col + 1).as_bytes(),
    );
    let look = arms.badge.take();
    let (bare_a, bare_b) = arms.draw(&mut t);
    arms.badge = look;
    let (a, b) = arms.draw(&mut t);
    let width = usize::try_from(arms.renderer.whole_size().0.value() + arms.margin.0).unwrap();
    let at = |v: i32| usize::try_from(v).unwrap();
    for y in at(row * ch)..at((row + 1) * ch) {
        for x in at(col * cw)..at((col + 1) * cw) {
            assert_eq!(
                a[y * width + x],
                bare_a[y * width + x],
                "instance arm, block at ({x}, {y})"
            );
            assert_eq!(
                b[y * width + x],
                bare_b[y * width + x],
                "cell arm, block at ({x}, {y})"
            );
        }
    }
    arms.badge = None;
    feed(&mut t, b"\x1b[2;40H\x1b[K\x1b[2 q\x1b[3;60H\x1b[K");
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
    // The cursor guide three lines back: on the cursor's line while that
    // line is in view, as iTerm2 draws it, the rows it changes the live
    // screen's lowered three rows, both arms agreeing; and on neither arm
    // once the line is below the window.
    let look = Some(baud_render::GuideLook {
        colour: baud_render::CURSOR_GUIDE,
        line: Px::of(2),
    });
    let width = usize::try_from(arms.renderer.whole_size().0.value() + arms.margin.0).unwrap();
    let ch = usize::try_from(arms.renderer.metrics().cell_h.value()).unwrap();
    let changed = |a: &[u32], b: &[u32]| -> Vec<usize> {
        (0..a.len() / width)
            .filter(|&y| a[y * width..(y + 1) * width] != b[y * width..(y + 1) * width])
            .collect()
    };
    feed(&mut t, b"\x1b7\x1b[?25h\x1b[2;5H");
    let guided = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, back: usize| {
        arms.back = Rows::of_count(back);
        arms.guide = None;
        let (bare_a, bare_b) = arms.draw(t);
        arms.guide = look;
        let (a, b) = arms.draw(t);
        arms.guide = None;
        assert_eq!(
            differing(&a, &b),
            0,
            "the arms disagree on the guide {back} back"
        );
        (changed(&a, &bare_a), changed(&b, &bare_b))
    };
    let (rest, _) = guided(&mut arms, &mut t, 0);
    assert!(
        !rest.is_empty(),
        "the control: the guide draws at the live screen"
    );
    let (ga, gb) = guided(&mut arms, &mut t, 3);
    let lowered: Vec<usize> = rest.iter().map(|y| y + 3 * ch).collect();
    assert_eq!(ga, lowered, "the instance arm's guide three lines back");
    assert_eq!(gb, lowered, "the cell arm's guide three lines back");
    feed(&mut t, b"\x1b[10;5H");
    let (ga, gb) = guided(&mut arms, &mut t, 3);
    assert!(
        ga.is_empty() && gb.is_empty(),
        "the cursor's line below the window"
    );
    feed(&mut t, b"\x1b8");
    arms.back = Rows::of_count(3);
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
    arms.highlight = Some(Highlight::new(
        (Row::from_index(1), Col::at_index(2)),
        (Row::from_index(4), Col::at_index(30)),
        Region::Stream,
    ));
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
    // **The selection in `OSC 17`'s and `19`'s colours**, each alone and
    // both, over the same rows and the block cursor inside them: the two
    // arms agree, each pair reaches the pixels, and `OSC 117` and `119` put
    // reverse video back. The black page is the 0-means-none sentinel's
    // edge: an opaque black is a set colour.
    let reverse = a;
    for (set, what) in [
        (&b"\x1b]17;#2f5f9f\x07"[..], "OSC 17"),
        (b"\x1b]117\x07\x1b]19;#ffd75f\x07", "OSC 19"),
        (b"\x1b]17;#000000\x07", "OSC 17 black and 19"),
    ] {
        feed(&mut t, set);
        let (a, b) = arms.draw(&mut t);
        assert_eq!(differing(&a, &b), 0, "the arms disagree under {what}");
        assert!(differing(&a, &reverse) > 0, "{what} reached the pixels");
    }
    feed(&mut t, b"\x1b]117\x07\x1b]119\x07");
    let (a, b) = arms.draw(&mut t);
    assert_eq!(differing(&a, &b), 0, "the arms disagree after the resets");
    assert_eq!(
        differing(&a, &reverse),
        0,
        "the resets put reverse video back"
    );
    // **The theme's selection colours**, the `Theme` selection setting: a
    // page and text the theme names are drawn alike by both arms, a
    // program's `OSC 17` wins over the theme's page, `OSC 117` puts the
    // theme's back, and the theme without them is reverse video again.
    let resting = arms.renderer.theme().clone();
    arms.renderer.set_theme(Theme {
        selection: Some(CellPaint::Colour(Argb(0xFF_33_4E_5E))),
        selection_text: Some(CellPaint::Colour(Argb(0xFF_FF_D7_5F))),
        ..resting.clone()
    });
    let (themed, b) = arms.draw(&mut t);
    assert_eq!(
        differing(&themed, &b),
        0,
        "the arms disagree under the theme's"
    );
    assert!(
        differing(&themed, &reverse) > 0,
        "the theme's reached the pixels"
    );
    feed(&mut t, b"\x1b]17;#2f5f9f\x07");
    let (a, b) = arms.draw(&mut t);
    assert_eq!(
        differing(&a, &b),
        0,
        "the arms disagree under OSC 17 over the theme's"
    );
    assert!(
        differing(&a, &themed) > 0,
        "OSC 17 won over the theme's page"
    );
    feed(&mut t, b"\x1b]117\x07");
    let (a, b) = arms.draw(&mut t);
    assert_eq!(
        differing(&a, &b),
        0,
        "the arms disagree after OSC 117 under the theme's"
    );
    assert_eq!(differing(&a, &themed), 0, "OSC 117 put the theme's back");
    arms.renderer.set_theme(resting.clone());
    let (a, _) = arms.draw(&mut t);
    assert_eq!(
        differing(&a, &reverse),
        0,
        "no theme colour is reverse video"
    );
    // **A window that is not key**: reverse video takes AppKit's
    // unemphasized pair, resolved under darkAqua, on both arms and in
    // pixels of its own; a theme's selection colours stay as they are; and
    // with the window key again it is reverse video.
    let pair = baud_present_macos::selection::unemphasized(
        &macos_liquid_glass::objc2_app_kit::NSAppearance::appearanceNamed(
            &macos_liquid_glass::objc2_foundation::NSString::from_str("NSAppearanceNameDarkAqua"),
        )
        .expect("darkAqua"),
    );
    arms.renderer.set_inactive_selection(Some(pair));
    let (inactive, b) = arms.draw(&mut t);
    assert_eq!(
        differing(&inactive, &b),
        0,
        "the arms disagree in a window that is not key"
    );
    assert!(
        differing(&inactive, &reverse) > 0,
        "the unemphasized pair reached the pixels"
    );
    assert!(
        inactive.contains(&0xFF_46_46_46),
        "the pair's grey is drawn"
    );
    arms.renderer.set_theme(Theme {
        selection: Some(CellPaint::Colour(Argb(0xFF_33_4E_5E))),
        selection_text: Some(CellPaint::Colour(Argb(0xFF_FF_D7_5F))),
        ..resting.clone()
    });
    let (a, b) = arms.draw(&mut t);
    assert_eq!(
        differing(&a, &b),
        0,
        "the arms disagree under the theme's, not key"
    );
    assert_eq!(differing(&a, &themed), 0, "the theme's stay as they are");
    arms.renderer.set_theme(resting.clone());
    arms.renderer.set_inactive_selection(None);
    let (a, _) = arms.draw(&mut t);
    assert_eq!(differing(&a, &reverse), 0, "key again: reverse video");
    // **xterm's special colours** (`OSC 5`, `OSC 6`): bold, underline and
    // reverse drawn in their own colours, under colorAttrMode over the
    // scene's own colours too, each attribute dropped where its colour took
    // it -- the face, the underline, the exchange -- in the one resolved
    // slot both arms read; and `OSC 106` and `105` put the scene back.
    let plain_again = a;
    // **A theme file's cell-relative selection and its cursor colours**
    // (alacritty's `CellForeground`/`CellBackground`, ghostty's
    // `cell-foreground`/`cell-background`): over the same rows, with the
    // block inside them and then outside, the two arms agree on each, each
    // reaches the pixels, and a theme's cursor text stays under a program's
    // `OSC 12`. The cell's colours exchanged by name are reverse video.
    let orange = Argb(0xFF_FF_88_00);
    let navy = Argb(0xFF_10_20_60);
    let looks = [
        (
            Theme {
                selection: Some(CellPaint::Colour(Argb(0xFF_33_4E_5E))),
                selection_text: Some(CellPaint::Page),
                ..resting.clone()
            },
            "a page with the cell's page as its text",
        ),
        (
            Theme {
                selection: Some(CellPaint::Page),
                selection_text: Some(CellPaint::Text),
                ..resting.clone()
            },
            "the cell's own colours as the selection's",
        ),
        (
            Theme {
                cursor: Some(CellPaint::Colour(orange)),
                cursor_text: Some(CellPaint::Colour(navy)),
                cursor_made: baud_render::CursorMade::Named,
                ..resting.clone()
            },
            "a theme's cursor fill and text",
        ),
        (
            Theme {
                cursor: Some(CellPaint::Page),
                cursor_text: Some(CellPaint::Text),
                cursor_made: baud_render::CursorMade::Named,
                ..resting.clone()
            },
            "the cell's own colours as the cursor's",
        ),
    ];
    for (look, what) in looks {
        arms.renderer.set_theme(look);
        for (at, place) in [(&b"\x1b[2;3H"[..], "inside"), (b"\x1b[8;5H", "outside")] {
            feed(&mut t, at);
            let (a, b) = arms.draw(&mut t);
            assert_eq!(
                differing(&a, &b),
                0,
                "the arms disagree under {what}, the block {place} the selection"
            );
            if place == "inside" {
                assert!(differing(&a, &reverse) > 0, "{what} reached the pixels");
            }
        }
        feed(&mut t, b"\x1b[2;3H\x1b]12;#00c000\x07");
        let (a, b) = arms.draw(&mut t);
        assert_eq!(
            differing(&a, &b),
            0,
            "the arms disagree under OSC 12 over {what}"
        );
        feed(&mut t, b"\x1b]112\x07");
        // kitty's `OSC 21`: a program's text under the block, and the
        // automatic colours over the theme's, on both arms.
        for program in [
            &b"\x1b]21;cursor_text=#c00000\x07"[..],
            b"\x1b]21;cursor=;cursor_text=;selection_background=;selection_foreground=\x07",
        ] {
            let (before, _) = arms.draw(&mut t);
            feed(&mut t, program);
            let (a, b) = arms.draw(&mut t);
            assert_eq!(
                differing(&a, &b),
                0,
                "the arms disagree under OSC 21 over {what}: {program:?}"
            );
            assert!(
                differing(&a, &before) > 0,
                "OSC 21 reached the pixels over {what}: {program:?}"
            );
        }
        feed(
            &mut t,
            b"\x1b]21;cursor;cursor_text;selection_background;selection_foreground\x07",
        );
    }
    arms.renderer.set_theme(Theme {
        selection: Some(CellPaint::Text),
        selection_text: Some(CellPaint::Page),
        ..resting.clone()
    });
    let (a, b) = arms.draw(&mut t);
    assert_eq!(
        differing(&a, &b),
        0,
        "the arms disagree under the exchange by name"
    );
    assert_eq!(
        differing(&a, &reverse),
        0,
        "the cell's colours exchanged by name are reverse video"
    );
    arms.renderer.set_theme(resting.clone());
    feed(
        &mut t,
        b"\x1b]5;0;#ff5f00;1;#5fff00;3;#af00ff\x07\x1b]6;0;1;1;1;3;1;5;1\x07",
    );
    let (a, b) = arms.draw(&mut t);
    assert_eq!(
        differing(&a, &b),
        0,
        "the arms disagree under the special colours"
    );
    assert!(
        differing(&a, &plain_again) > 0,
        "the special colours reached the pixels"
    );
    feed(&mut t, b"\x1b]106;0;0;1;0;3;0;5;0\x07\x1b]105\x07");
    let (a, b) = arms.draw(&mut t);
    assert_eq!(differing(&a, &b), 0, "the arms disagree after OSC 106");
    assert_eq!(
        differing(&a, &plain_again),
        0,
        "OSC 106 and 105 put the scene back"
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
    arms.highlight = Some(Highlight::new(corners.0, corners.1, Region::Block));
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
    arms.highlight = Some(Highlight::new(corners.0, corners.1, Region::Stream));
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
        measure: baud_render::Measure::default(),
        hints: Vec::new(),
        caret: Caret::End,
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
    // **And under an input method's hints**: a selected cluster's page, an
    // italic and a faint one, the curly and the struck line, each drawn
    // by records both arms take.
    use baud_render::Hints;
    if let Some(c) = arms.composition.as_mut() {
        c.hints = vec![
            Hints::SELECTION.with(Hints::WHOLE),
            Hints::PREDICTION.with(Hints::SPELLING_ERROR),
            Hints::PREFIX.with(Hints::COMPOSE_ERROR),
            Hints::NONE,
        ];
    }
    let (hint_a, hint_b) = arms.draw(&mut t);
    assert_eq!(
        differing(&hint_a, &hint_b),
        0,
        "the arms disagree over a composition's hints"
    );
    assert!(
        differing(&hint_a, &comp_a) > 0,
        "the hints reached the pixels"
    );
    // **And the input method's cursor inside it**: on its second cluster
    // in the block and in a bar, a range drawn selected with no cursor, and
    // none -- each the same in both arms and each a change.
    for (style, caret) in [
        (&b"\x1b[2 q"[..], Caret::At(1)),
        (&b"\x1b[6 q"[..], Caret::At(1)),
        (&b"\x1b[2 q"[..], Caret::Range(1, 3)),
        (&b"\x1b[2 q"[..], Caret::Hidden),
    ] {
        feed(&mut t, style);
        if let Some(c) = arms.composition.as_mut() {
            c.hints = Vec::new();
            c.caret = caret;
        }
        let (a, b) = arms.draw(&mut t);
        assert_eq!(
            differing(&a, &b),
            0,
            "the arms disagree over the input method's cursor, {caret:?}"
        );
        assert!(differing(&a, &comp_a) > 0, "{caret:?} reached the pixels");
    }
    feed(&mut t, b"\x1b[0 q");
    arms.composition = None;
    // **The find field and every match's wash**: the field over the top
    // row's right end in the selection's colours with its caret, and a
    // yellow wash over each match but the current one, the selection's.
    // The cell arm masks the field's cells and appends the solids to its
    // records; the instance arm converts the cells blank.
    let mut find = baud_render::Find::new();
    find.set_query(
        &t,
        "e".chars().map(baud_domain::Codepoint::from_char).collect(),
        arms.back,
    );
    assert!(find.matches().len() > 2, "the scene holds matches");
    arms.find = Some(baud_render::FindLook {
        marks: find.marks(&t, arms.back),
        field: find.field(t.geometry().0),
    });
    let (find_a, find_b) = arms.draw(&mut t);
    assert_eq!(
        differing(&find_a, &find_b),
        0,
        "the arms disagree over the find field and its marks"
    );
    assert!(
        differing(&find_a, &plain) > 0,
        "the find reached the pixels"
    );
    // **An input method composing in the field**, the query selected: the
    // composition after it in its hints, an underlined prediction on the
    // first and none on the second, the query in the grid's colours, the
    // caret at the composition's cursor -- both arms alike, and a change
    // to the field's pixels.
    let kana: Vec<_> = "かな"
        .chars()
        .map(baud_domain::Codepoint::from_char)
        .collect();
    arms.find = Some(baud_render::FindLook {
        marks: find.marks(&t, arms.back),
        field: find.field_typing(
            t.geometry().0,
            baud_render::Typing {
                query: baud_render::Query::Selected,
                composing: &kana,
                hints: &[baud_render::Hints::WHOLE.with(baud_render::Hints::PREDICTION)],
                caret: baud_render::Caret::At(1),
            },
        ),
    });
    let (typing_a, typing_b) = arms.draw(&mut t);
    assert_eq!(
        differing(&typing_a, &typing_b),
        0,
        "the arms disagree over the composition in the field"
    );
    assert!(
        differing(&typing_a, &find_a) > 0,
        "the composition reached the field's pixels"
    );
    arms.find = None;
    // **The link preview**: a run of cells underlined on two rows and the
    // address bar on the bottom row, as a field. The cell
    // arm masks the bar's cells and appends the underline to its records;
    // the instance arm converts the cells blank.
    let text: Vec<_> = " https://example.org/docs "
        .chars()
        .map(baud_domain::Codepoint::from_char)
        .collect();
    let n = text.len();
    let last = Row::from_index(t.geometry().1.index() - 1);
    arms.link = Some(baud_render::LinkLook {
        spans: vec![
            baud_render::LinkSpan {
                row: Row::from_index(1),
                from: Col::at_index(3),
                to: Col::at_index(12),
            },
            baud_render::LinkSpan {
                row: Row::from_index(2),
                from: Col::at_index(0),
                to: Col::at_index(4),
            },
        ],
        bar: baud_render::Field {
            row: last,
            col: Col::ZERO,
            text,
            hints: vec![baud_render::Hints::SELECTION; n],
            caret: Col::at_index(n),
        },
    });
    let (link_a, link_b) = arms.draw(&mut t);
    assert_eq!(
        differing(&link_a, &link_b),
        0,
        "the arms disagree over the link preview"
    );
    assert!(
        differing(&link_a, &plain) > 0,
        "the link preview reached the pixels"
    );
    arms.link = None;
    // **The hover tip over DEC's line**: CD's text in lines of the
    // selection's colours over the bottom rows, right-aligned. The cell arm
    // masks its cells as it masks the bar's; the instance arm converts
    // them blank; the CPU arm, the Linux driver's, draws the same text.
    let (cols, rows) = t.geometry();
    arms.tip = baud_render::tip_look(baud_render::Led::Cd.tip(), cols, rows);
    assert!(arms.tip.is_some(), "room for CD's tip");
    let (tip_a, tip_b) = arms.draw(&mut t);
    assert_eq!(
        differing(&tip_a, &tip_b),
        0,
        "the arms disagree over the hover tip"
    );
    assert!(differing(&tip_a, &plain) > 0, "the tip reached the pixels");
    // The CPU arm, which the Linux driver draws with, against the GPU arms
    // the macOS app draws with: the same pixels, with the tip and without.
    arms.tip = None;
    let (plain_a, _) = arms.draw(&mut t);
    let plain_cpu = cpu_frame(&mut arms, &mut t);
    assert_eq!(channel_diff(&plain_cpu, &plain_a, |_| true), (0, 0));
    arms.tip = baud_render::tip_look(baud_render::Led::Cd.tip(), cols, rows);
    let (tip_a, _) = arms.draw(&mut t);
    let tip_cpu = cpu_frame(&mut arms, &mut t);
    assert_eq!(
        channel_diff(&tip_cpu, &tip_a, |_| true),
        (0, 0),
        "the CPU arm draws the tip as the GPU arms do"
    );
    assert_ne!(
        differing(&tip_cpu, &plain_cpu),
        0,
        "the CPU arm drew the tip"
    );
    arms.tip = None;
    let (gone_a, _) = arms.draw(&mut t);
    assert_eq!(differing(&gone_a, &plain), 0, "the tip leaves no trace");
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
        strength: baud_render::Strength::Full,
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
        strength: baud_render::Strength::Full,
        opacity: Smear::OPAQUE,
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
        strength: baud_render::Strength::Full,
        opacity: Smear::OPAQUE,
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
        strength: baud_render::Strength::Full,
        opacity: Smear::OPAQUE,
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
            strength: baud_render::Strength::Full,
            opacity: Smear::OPAQUE,
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
            strength: baud_render::Strength::Full,
            opacity: Smear::OPAQUE,
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
    repeat_pass_gate(&mut arms, &mut t);
    arms.trail = None;
    // The alternate-screen fade: a full-screen veil at mid-progress, drawn
    // by both arms as one translucent solid over everything, and a change to
    // the pixels against the unveiled frame it was drawn over.
    let (unveiled, _) = arms.draw(&mut t);
    arms.fade = Some(Transition::fade(0.5));
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
    arms.fade = Some(Transition::fade(0.5));
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
            measure: baud_render::Measure::default(),
            hints: Vec::new(),
            caret: Caret::End,
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
            measure: baud_render::Measure::default(),
            hints: Vec::new(),
            caret: Caret::End,
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

    // A marked cell on the leaving row draws its cluster there, as on a
    // row of the window: the walk over that row masks the cell and a
    // record draws the letter and its mark (`Overlay::leaving`), where the
    // row drawn from the map alone showed the base without the mark. A
    // full-row slide is the screen one line up drawn still, and against the
    // same screen with the mark never sent it is not, so the mark reached
    // the pixels; another letter there draws another frame. The letter is
    // `Q`, which nothing before this draws, so its entry is made by the
    // walk over the leaving row.
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
        arms.back = Rows::of_count(1);
        let (still, _) = arms.draw(t);
        arms.back = Rows::ZERO;
        (above, frame, still)
    };
    let (marked, with_mark, still) = leaving(&mut t, "Q\u{0301}");
    let (bare, without, _) = leaving(&mut t, "Q");
    let (_, other, _) = leaving(&mut t, "x");
    assert!(
        marked.flags.is_marked() && !bare.flags.is_marked(),
        "the premise: the leaving row's first cell carries the mark, then not"
    );
    assert_eq!(
        differing(&with_mark, &still),
        0,
        "a marked cell on the leaving row draws as the row drawn still"
    );
    assert!(
        differing(&with_mark, &without) > 0,
        "the mark on the leaving row reached the pixels"
    );
    assert!(
        differing(&without, &other) > 0,
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
        strength: baud_render::Strength::Full,
        opacity: Smear::OPAQUE,
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
            strength: baud_render::Strength::Full,
            opacity: Smear::OPAQUE,
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
    // shifted *and* a shader change. That is why rank 1 stops at the
    // on-screen rows.
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
    // The page a reversed default background is: raised above the dim to
    // the alpha its text reads at (`Theme::reversed_alpha`).
    let raised = resting.reversed_alpha(resting.fg, resting.bg).get();
    assert_eq!(alpha(4, 2), raised, "?5: the fifteen blanks draw the page");
    assert!(raised > 56, "the premise: the page is raised");
    feed(&mut t, b"\x1b[?5l");
    arms.renderer.set_theme(resting.clone());
    for case in glass_cases(&resting) {
        arms.renderer.set_theme(case.resting.clone());
        glass_floor_gate(&mut arms, &mut t, &case);
    }
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
    // baud's own bars on row 0, over the image's top rows once it is
    // placed: here first with no image under them.
    let text: Vec<_> = " https://example.org/docs "
        .chars()
        .map(baud_domain::Codepoint::from_char)
        .collect();
    let n_bar = text.len();
    let bar = baud_render::Field {
        row: Row::ZERO,
        col: Col::ZERO,
        text,
        hints: vec![baud_render::Hints::SELECTION; n_bar],
        caret: Col::at_index(n_bar),
    };
    let link = baud_render::LinkLook {
        spans: Vec::new(),
        bar: bar.clone(),
    };
    let find = baud_render::FindLook {
        marks: Vec::new(),
        field: bar,
    };
    arms.link = Some(link.clone());
    let (_, link_bare) = arms.draw(&mut t);
    arms.link = None;
    arms.find = Some(find.clone());
    let (_, find_bare) = arms.draw(&mut t);
    arms.find = None;
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
    // **baud's own overlays over the image**: the link preview's bar and
    // the find field on row 0, over the image's top, are what they are
    // with no image there, in both arms; the fade veils the image too.
    // Perturbation: draw the images over the text after every record
    // again, in either arm -- the bar's cells are the image's.
    let (cw, ch) = (
        usize::try_from(arms.renderer.metrics().cell_w.value()).unwrap(),
        usize::try_from(arms.renderer.metrics().cell_h.value()).unwrap(),
    );
    let bar_px = |px: &[u32]| -> Vec<u32> {
        (0..ch)
            .flat_map(|y| (0..n_bar * cw).map(move |x| y * iw + x))
            .map(|i| px[i])
            .collect()
    };
    assert_ne!(
        bar_px(&img_b),
        bar_px(&link_bare),
        "the image is under the bar"
    );
    for (what, bare) in [("the link bar", &link_bare), ("the find field", &find_bare)] {
        match what {
            "the link bar" => arms.link = Some(link.clone()),
            _ => arms.find = Some(find.clone()),
        }
        let (a, b) = arms.draw(&mut t);
        (arms.link, arms.find) = (None, None);
        assert_eq!(
            differing(&a, &b),
            0,
            "the arms disagree on {what} over the image"
        );
        assert!(bar_px(&a) == bar_px(bare), "{what} is drawn over the image");
    }
    arms.fade = Some(Transition::fade(0.5));
    let (a, b) = arms.draw(&mut t);
    arms.fade = None;
    assert_eq!(
        differing(&a, &b),
        0,
        "the arms disagree on the fade over the image"
    );
    let rect = |px: &[u32]| -> Vec<u32> {
        (ry..ry + rh)
            .flat_map(|y| (rx..rx + rw).map(move |x| y * iw + x))
            .map(|i| px[i])
            .collect()
    };
    assert!(rect(&a) != rect(&img_b), "the fade veils the image");

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
    sixel_fill_gate(&mut arms, &mut t);
    sixel_recolour_gate(&mut arms, &mut t);
    hollow_cursor_gate(&mut arms, &mut t);
    cpu_page_gate(&mut arms, &mut t);
    status_marks_gate(&mut arms, &mut t);
    wrap_marks_gate(&mut arms, &mut t);
    lights_line_gate(&mut arms, &mut t);
    lights_line_slide_gate(&mut arms, &mut t);
    substitute_mark_gate(&mut arms, &mut t);
    kitty_draw_gate(&mut arms, &mut t);
    iterm_file_gate(&mut arms, &mut t);
    iterm_jpeg_gate(&mut arms, &mut t);
    iterm_gif_gate(&mut arms, &mut t);
    kitty_per_cell_gate(&mut arms, &mut t);
    decoration_gate(&mut arms, &mut t);
    proportional_gate(&mut arms, &mut t);
    line_rendition_gate(&mut arms, &mut t);
    ambiguous_box_gate(&mut arms, &mut t);
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
        arms.highlight = Some(Highlight::new(
            (Row::from_index(1), Col::at_index(0)),
            (Row::from_index(1), Col::at_index(3)),
            Region::Stream,
        ));
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
    // **A variation selector picks the face.** JetBrains
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

/// **`SGR 53` overlined, `51` framed, the ideogram lines `60-63` and
/// `64`'s stress mark on all three arms**: the overline over text and over
/// spaces, beside a straight, a curly and a double underline and an `SGR
/// 58` colour, both double ideogram lines, framed runs that start at the
/// left edge, end at the right edge, end at a wide character, and touch an
/// overline, a selected framed run and a framed run in blink and conceal,
/// encircled runs alone and in a run, framed, wide, reversed, blinking, over
/// an overline, a set background and a stress mark, at either edge,
/// selected and slid, and a hidden neighbour of each outline,
/// and stress marks over narrow and wide characters, punctuation, spaces
/// and an ideographic space, under an overline, reversed, blinking and
/// concealed. The instance and cell arms must agree pixel for pixel and
/// the CPU arm with them, and a positive control: the same text without
/// the renditions paints differently, so two arms that both dropped them
/// cannot agree here vacuously.
fn decoration_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    let cols = usize::from(WS.ws_col);
    let edge = format!(
        "\x1b[1;{}H\x1b[51mend\x1b[0m\x1b[7;{}H\x1b[52mfin\x1b[0m",
        cols - 2,
        cols - 2
    );
    let lines = [
        "\x1b[53mover\x1b[0m \x1b[53;4mover under\x1b[0m \x1b[53;4:3mcurl\x1b[0m \
         \x1b[53;21mdouble\x1b[0m \x1b[58;5;196;53;4mul red\x1b[0m \x1b[31;53m    \x1b[0m",
        "\x1b[60mideo under\x1b[0m \x1b[61mideo double\x1b[0m \x1b[62mideo over\x1b[0m \
         \x1b[63mideo double over\x1b[0m \x1b[60;62mboth\x1b[0m",
        "\x1b[51mframed\x1b[0m \x1b[51;53mframe over\x1b[0m \x1b[51mwide \u{231a}\x1b[0m\
         \x1b[51;32m a b \x1b[0m\x1b[51;44mset bg\x1b[0m \x1b[51;5mblink\x1b[0m \
         \x1b[51;8mhid\x1b[0m \x1b[51;7mrev\x1b[0m",
        "\x1b[64mstress \u{4e2d}\u{6587}, a.b\x1b[0m \x1b[64;53mover\x1b[0m \
         \x1b[64;7mrev\x1b[0m \x1b[64;5mblink\x1b[0m \x1b[64;8mhid\x1b[0m \
         \x1b[64;34m\u{3000}x\x1b[0m \x1b[64me\u{301},\u{301}\u{203c}\u{fe0f} \u{301} ,\u{301}->\x1b[0m",
        "\x1b[52mok\x1b[0m \x1b[52mA B C\x1b[0m \x1b[52m1\x1b[0m \x1b[51;52mboth\x1b[0m \
         \x1b[52mwide \u{231a}\x1b[0m \x1b[52;7mrev\x1b[0m \x1b[52;5mblink\x1b[0m \
         \x1b[52;53mover\x1b[0m \x1b[52;44mbg\x1b[0m \x1b[52;64mstress\x1b[0m \
         \x1b[52me\u{301}\x1b[0m \x1b[52m->\x1b[0m \x1b[51mx\u{301}y\x1b[0m",
    ];
    let mut text = String::from("\x1bc\x1b[?25l\x1b[51mstart\x1b[0m\r\n");
    for l in lines {
        text.push_str(l);
        text.push_str("\r\n");
    }
    text.push_str(&edge);
    // The same characters and colours with every decoration taken out.
    let plain = text
        .replace("51;", "")
        .replace("53;", "")
        .replace("\x1b[53m", "")
        .replace("\x1b[51m", "")
        .replace(";53m", "m")
        .replace("\x1b[60m", "")
        .replace("\x1b[61m", "")
        .replace("\x1b[62m", "")
        .replace("\x1b[63m", "")
        .replace("\x1b[60;62m", "")
        .replace("\x1b[64m", "")
        .replace("64;", "")
        .replace("52;", "")
        .replace("\x1b[52m", "");
    // The text with only the encircled runs taken out: a control of their
    // own, as the stress marks have.
    let unringed = text
        .replace("52;", "")
        .replace(";52m", "m")
        .replace("\x1b[52m", "");
    // The text with only the stress marks taken out: a control of their
    // own, since the other decorations would satisfy the one above.
    let unstressed = text.replace("\x1b[64m", "").replace("64;", "");
    feed(t, plain.as_bytes());
    let (none, _) = arms.draw(t);
    feed(t, unstressed.as_bytes());
    let (bare, _) = arms.draw(t);
    feed(t, unringed.as_bytes());
    let (unringed, _) = arms.draw(t);
    feed(t, text.as_bytes());
    let (a, b) = arms.draw(t);
    assert!(
        differing(&none, &a) > 0,
        "the decorations never reached the instance arm's pixels"
    );
    assert!(
        differing(&unringed, &a) > 0,
        "the encircled runs never reached the instance arm's pixels"
    );
    assert!(
        differing(&bare, &a) > 0,
        "the stress marks never reached the instance arm's pixels"
    );
    let cpu = cpu_frame(arms, t);
    assert_eq!(
        differing(&a, &b),
        0,
        "the GPU arms disagree on the decorations"
    );
    assert_eq!(
        differing(&a, &cpu),
        0,
        "the CPU arm disagrees on the decorations"
    );
    // A selection across a framed run and an encircled one swaps their
    // colours on both arms.
    arms.highlight = Some(Highlight::new(
        (Row::from_index(3), Col::at_index(2)),
        (Row::from_index(5), Col::at_index(20)),
        Region::Stream,
    ));
    let (a, b) = arms.draw(t);
    arms.highlight = None;
    assert_eq!(
        differing(&a, &b),
        0,
        "the GPU arms disagree on a selected frame"
    );
    // A full-row slide is the screen one line up, encircled runs and all:
    // the cell arm places a ring's mask by the pan as it places a glyph.
    let rows = usize::from(WS.ws_row);
    feed(t, format!("\x1b[{rows};1H\n").as_bytes());
    let cell_h = arms.renderer.metrics().cell_h;
    arms.back = Rows::ZERO;
    arms.slide = Some(cell_h);
    let (_, slid) = arms.draw(t);
    arms.slide = None;
    arms.back = Rows::of_count(1);
    let (_, one_up) = arms.draw(t);
    arms.back = Rows::ZERO;
    assert_eq!(
        differing(&slid, &one_up),
        0,
        "a full-row slide over the decorations is not the screen one line up"
    );
    hidden_seam_gate(arms, t);
    reach_gate(arms, t);
    overlap_gate(arms, t);
    composed_decoration_gate(arms, t);
}

/// **A `BEL` in a colour is drawn alike on every path**: the instance and
/// cell arms and the CPU arm draw the bell's solid over the cells and under
/// the thumb, at the peak and on the way down, on the dark and the light
/// theme -- and each frame differs from the one without it, so equal frames
/// are not three blanks; with no colour set the frame is the other video's,
/// the solid absent.
#[test]
fn a_coloured_bell_draws_alike_on_every_arm() {
    let data = std::fs::read(format!(
        "{}/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("face");
    let faces = [Face::parse(&data).expect("parse")];
    for theme in [Theme::dark(), Theme::light()] {
        let fresh = || {
            Renderer::new(
                Stack::new(&faces).unwrap(),
                PxSize::of(14.0).unwrap(),
                theme.clone(),
                Blend::Native,
                Cols::of_index(usize::from(WS.ws_col)),
                Rows::of_index(usize::from(WS.ws_row)),
            )
            .unwrap()
        };
        arms!(fresh() => arms, t);
        feed(
            &mut t,
            b"\x1b[?25l\x1b[2;3HThe quick brown fox \x1b[31mjumps over\x1b[0m 0123456789\x1b[7m reversed \x1b[0m",
        );
        let (plain, _) = arms.draw(&mut t);
        for (colour, at) in [(&b"#ff0000"[..], 50), (b"#3f638b", 50), (b"#ffffff", 80)] {
            let what = format!("{} at {at} ms", String::from_utf8_lossy(colour));
            feed(
                &mut t,
                &[&b"\x1b]21;visual_bell="[..], colour, b"\x07"].concat(),
            );
            let kind = BellKind::of(&t);
            let (b, _) =
                BellFlash::new(Duration::from_millis(100)).at(Duration::ZERO, Rang::Yes, kind);
            let shown = b.at(Duration::from_millis(at), Rang::No, kind).1;
            assert!(shown.tint().is_some(), "{what}: a tint");
            arms.renderer.set_bell_tint(shown.tint());
            let (a, b) = arms.draw(&mut t);
            let cpu = cpu_frame(&mut arms, &mut t);
            assert_eq!(a.len(), cpu.len(), "{what}: one frame size");
            assert!(
                differing(&plain, &a) > 0,
                "{what}: the tint reached the pixels"
            );
            assert_eq!(differing(&a, &b), 0, "{what}: the cell arm differs");
            assert_eq!(differing(&a, &cpu), 0, "{what}: the CPU arm differs");
        }
        arms.renderer.set_bell_tint(None);
        let (back, _) = arms.draw(&mut t);
        assert_eq!(differing(&back, &plain), 0, "nothing left of the tint");
        feed(&mut t, b"\x1b]21;visual_bell\x07");
        let (b, shown) = BellFlash::new(Duration::from_millis(100)).at(
            Duration::ZERO,
            Rang::Yes,
            BellKind::of(&t),
        );
        assert_eq!(shown, BellShown::Video, "no colour: the other video");
        assert!(b.lit().is_some());
        arms.renderer.set_flash(shown.video());
        arms.renderer.set_bell_tint(shown.tint());
        let (a, b) = arms.draw(&mut t);
        assert_eq!(differing(&a, &b), 0, "the arms disagree on the other video");
        assert!(
            differing(&a, &plain) > 0,
            "the other video reached the pixels"
        );
        arms.renderer.set_flash(VideoFlash::Off);
    }
}

/// **The shape pass drawn by what the table holds** (`Gpu::draw_cells`
/// leaves it undrawn while no slot carries a decoration, but for the
/// cursor's line): each decoration alone, minted on a screen whose table
/// held none, and an undecorated screen under each cursor shape, shaped and
/// hollow, solid and part way through a fade -- each on a terminal of its
/// own, since a reset keeps the table's slots. The cell arm against the
/// instance arm and the CPU arm, with the premise checked each time -- the
/// table held no decoration before, and the decoration or the cursor
/// reached the pixels -- so a case cannot pass by testing nothing.
#[test]
fn each_decoration_alone_and_the_cursor_line_draw_alike_on_every_arm() {
    let data = std::fs::read(format!(
        "{}/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("face");
    let faces = [Face::parse(&data).expect("parse")];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    // The screen before the case, drawn: the table it was synced from holds
    // no decoration.
    let plain = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, what: &str| {
        feed(
            t,
            b"\x1b[?25l\x1b[2;3HThe quick brown fox \x1b[31mjumps over\x1b[0m 0123456789",
        );
        let (before, _) = arms.draw(t);
        assert_eq!(
            arms.stamp.decorations(),
            baud_render::Decorations::Absent,
            "{what}: the table held no decoration before"
        );
        before
    };
    let tail = |sgr: &str| format!(" \x1b[{sgr}mmarked, a.b\x1b[0m end");
    for sgr in [
        "4", "9", "21", "4:2", "4:3", "4:4", "4:5", "53", "62", "63", "51", "52", "64",
    ] {
        let what = format!("SGR {sgr} alone");
        let control = {
            arms!(fresh() => arms, t);
            plain(&mut arms, &mut t, &what);
            feed(&mut t, tail("0").as_bytes());
            arms.draw(&mut t).0
        };
        arms!(fresh() => arms, t);
        plain(&mut arms, &mut t, &what);
        feed(&mut t, tail(sgr).as_bytes());
        let (a, b) = arms.draw(&mut t);
        assert_eq!(
            arms.stamp.decorations(),
            baud_render::Decorations::Present,
            "{what}: counted a decoration"
        );
        assert!(
            differing(&control, &a) > 0,
            "{what}: never reached the instance arm's pixels"
        );
        assert_eq!(differing(&a, &b), 0, "{what}: the cell arm differs");
        let cpu = cpu_frame(&mut arms, &mut t);
        assert_eq!(differing(&a, &cpu), 0, "{what}: the CPU arm differs");
    }
    // The cursor on a table with no decoration, its own line the pass's one
    // instance: under every shape and look, solid and fading.
    for (shape, style) in [("block", 2), ("underline", 4), ("bar", 6)] {
        for look in [CursorLook::Shaped, CursorLook::Hollow] {
            for phase in [None, Some(blink_phases()[0].0)] {
                let what = format!("a {shape} cursor, {look:?}, fading {}", phase.is_some());
                arms!(fresh() => arms, t);
                arms.renderer.set_cursor_look(look);
                plain(&mut arms, &mut t, &what);
                feed(&mut t, tail("0").as_bytes());
                let (none, _) = arms.draw(&mut t);
                feed(
                    &mut t,
                    format!("\x1b[{style} q\x1b[2;5H\x1b[?25h").as_bytes(),
                );
                if let Some(p) = phase {
                    arms.renderer.set_cursor_blink(p);
                }
                let (a, b) = arms.draw(&mut t);
                assert_eq!(
                    arms.stamp.decorations(),
                    baud_render::Decorations::Absent,
                    "{what}: the table holds no decoration"
                );
                assert!(
                    differing(&none, &a) > 0,
                    "{what}: the cursor reached no pixel"
                );
                assert_eq!(differing(&a, &b), 0, "{what}: the cell arm differs");
                let cpu = cpu_frame(&mut arms, &mut t);
                let d = channel_diff(&cpu, &a, |_| true);
                assert_eq!(d.0, 0, "{what}: the CPU arm differs by up to {}", d.1);
            }
        }
    }
}

/// **The cursor's line alone, drawn by rows**: an underline and a bar
/// cursor on a table with no decoration, moved and blinked by
/// cursor-only frames, each drawn into the target the rotation gives it
/// and compared with the whole frame -- the one instance of the shape pass
/// held to each band's scissor as the whole pass would be.
#[test]
fn an_undecorated_cursor_line_draws_by_rows_as_the_whole_frame_does() {
    for shape in ["\x1b[4 q", "\x1b[6 q"] {
        let mut steps: Vec<Step> = vec![(
            "plain text, the cursor a line",
            Damage::Incremental,
            Box::new(move |_, t| {
                feed(
                    t,
                    b"\x1b[?25h\x1b[2;1HThe quick brown fox jumps over 0123456789",
                );
                feed(
                    t,
                    b"\x1b[5;1H\x1b[32mthe lazy dog\x1b[0m, again: ABCDEFGHIJ",
                );
                feed(t, format!("{shape}\x1b[3;5H").as_bytes());
            }),
            Want::Any,
        )];
        let cells = [
            (3, 9),
            (8, 30),
            (4, 1),
            (2, 3),
            (1, 1),
            (9, 119),
            (5, 60),
            (10, 2),
        ];
        for (r, c) in cells.iter().copied().chain(cells.iter().copied().rev()) {
            steps.push((
                "the cursor moved",
                Damage::Incremental,
                at(r, c),
                Want::Rows,
            ));
        }
        for p in [CursorPhase::HIDDEN, CursorPhase::SHOWN].repeat(3) {
            steps.push((
                "the blink flipped",
                Damage::Incremental,
                blink(p),
                Want::Rows,
            ));
        }
        steps.push((
            "the table still holds no decoration",
            Damage::Incremental,
            Box::new(|a, t| {
                assert_eq!(
                    a.stamp.decorations(),
                    baud_render::Decorations::Absent,
                    "the premise: the shape pass draws the cursor's line alone"
                );
                feed(t, b"\x1b[6;7H");
            }),
            Want::Rows,
        ));
        let n = steps.len();
        rows_were_few(&cell_oracle(Reads::Copy, Px::of(0), steps, n));
    }
}

/// **A `BEL`'s tint redraws the whole frame on a surface drawn before**:
/// its solid changes with no cell, so each frame of its fade, and the one
/// it leaves, is drawn whole into the target the rotation gives it and
/// equals the frame drawn whole -- on the cell and the instance paths --
/// and a cursor move after it is drawn by rows again.
#[test]
fn a_bell_tint_redraws_whole_on_a_reused_surface() {
    let tint = |alpha: Option<u8>| -> Act {
        Box::new(move |a, _| {
            a.renderer
                .set_bell_tint(alpha.map(|v| baud_render::BellTint {
                    colour: Argb(0xFFFF_0000),
                    alpha: Chan::from_u8(v),
                }))
        })
    };
    let steps = || -> Vec<Step> {
        vec![
            (
                "plain text",
                Damage::Incremental,
                Box::new(|_, t| {
                    feed(
                        t,
                        b"\x1b[?25h\x1b[2;1HThe quick brown fox jumps over 0123456789\x1b[3;5H",
                    );
                }),
                Want::Any,
            ),
            (
                "the tint rises",
                Damage::Incremental,
                tint(Some(36)),
                Want::Whole,
            ),
            (
                "the tint peaks",
                Damage::Incremental,
                tint(Some(153)),
                Want::Whole,
            ),
            (
                "the tint falls",
                Damage::Incremental,
                tint(Some(36)),
                Want::Whole,
            ),
            (
                "the tint goes",
                Damage::Incremental,
                tint(None),
                Want::Whole,
            ),
            (
                "the cursor moved",
                Damage::Incremental,
                at(4, 9),
                Want::Rows,
            ),
        ]
    };
    cell_oracle(Reads::Copy, Px::of(0), steps(), 0);
    instance_oracle(Px::of(0), steps(), 0);
}

/// **The cursor's line under a slide**: a frame sliding by a whole row
/// with an underline or a bar cursor, drawn from a table with no
/// decoration and from one that holds an underline no cell names -- the
/// cursor's one instance against the whole pass, on screens alike.
#[test]
fn a_sliding_cursor_line_draws_as_the_whole_pass_does() {
    let data = std::fs::read(format!(
        "{}/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("face");
    let faces = [Face::parse(&data).expect("parse")];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    for shape in ["\x1b[4 q", "\x1b[6 q"] {
        let frames = ["\x1b[1;1Hy", "\x1b[1;1H\x1b[4mx\x1b[0m\x1b[1;1Hy"].map(|before| {
            arms!(fresh() => arms, t);
            feed(&mut t, before.as_bytes());
            let rows = usize::from(WS.ws_row);
            for y in 1..=rows {
                feed(
                    &mut t,
                    format!("\x1b[{y};3Hline {y}: The quick brown fox, 0123456789").as_bytes(),
                );
            }
            feed(
                &mut t,
                format!("\x1b[{rows};1H\n{shape}\x1b[5;9H").as_bytes(),
            );
            let cell_h = arms.renderer.metrics().cell_h;
            feed(&mut t, b"\x1b[?25l");
            arms.slide = Some(cell_h);
            let (_, none) = arms.draw(&mut t);
            feed(&mut t, b"\x1b[?25h");
            let (_, slid) = arms.draw(&mut t);
            assert!(
                differing(&none, &slid) > 0,
                "{shape:?} after {before:?}: the cursor reached the slide's pixels"
            );
            (arms.stamp.decorations(), slid)
        });
        let [(absent, a), (present, b)] = frames;
        assert_eq!(absent, baud_render::Decorations::Absent, "the premise");
        assert_eq!(present, baud_render::Decorations::Present, "the premise");
        assert_eq!(
            differing(&a, &b),
            0,
            "{shape:?}: the cursor's one instance is not the whole pass's line"
        );
    }
}

/// **`SGR 11`-`19` and `SGR 20` draw from their fonts alike on all three
/// arms**: the cell arm through the font bits in its glyph-map key, the
/// instance and CPU arms through the stack. Font 1 is JetBrains Mono
/// Italic and font 9 its Bold Italic, each named alone, with font 1's Bold
/// beside it, and the Fraktur face's stand-in is its Bold; the scene draws
/// each named font over text, a ligature, a wide character, a marked
/// cluster (a record of its own) and a Nerd Fonts icon no text face
/// carries, which comes from the chain behind the font, bold in font 1,
/// font 2, which no face is named for and which draws the primary, and
/// Fraktur alone and over font 1. The premise that the fonts reached the
/// pixels: the same text in font 10 paints otherwise, the Fraktur lines
/// alone paint otherwise, and font 2's line paints what font 10's does.
#[test]
fn alternative_fonts_draw_alike_on_every_arm() {
    let app_font = |p: &str| {
        std::fs::read(format!(
            "{}/../baud-app/fonts/{p}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("face")
    };
    let regular = app_font("jetbrains-mono/JetBrainsMono-Regular.ttf");
    let symbols = app_font("symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf");
    let italic = app_font("jetbrains-mono/JetBrainsMono-Italic.ttf");
    let bold = app_font("jetbrains-mono/JetBrainsMono-Bold.ttf");
    let bold_italic = app_font("jetbrains-mono/JetBrainsMono-BoldItalic.ttf");
    let faces = [
        Face::parse(&regular).expect("parse"),
        Face::parse(&symbols).expect("parse"),
        Face::parse(&italic).expect("parse"),
        Face::parse(&bold).expect("parse"),
        Face::parse(&bold_italic).expect("parse"),
        Face::parse(&bold).expect("parse"),
    ];
    let font = |n: u16| FontSlot::alternative(baud_domain::FontId::from_sgr(10 + n).unwrap());
    let roles = [
        FaceRole::Fallback,
        FaceRole::Fallback,
        FaceRole::Font(font(1), FaceStyle::Regular),
        FaceRole::Font(font(1), FaceStyle::Bold),
        FaceRole::Font(font(9), FaceStyle::Regular),
        FaceRole::Font(FontSlot::FRAKTUR, FaceStyle::Regular),
    ];
    let renderer = Renderer::new(
        Stack::with_roles(&faces, &roles).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    arms!(renderer => arms, t);
    let body = "ab->x \u{4e2d}\u{6587} e\u{301}q \u{f07b} WiW the quick brown fox jumps 0123";
    let lines = |sgr: [&str; 6]| {
        let mut out = String::from("\x1bc\x1b[?25l");
        for (i, s) in sgr.iter().enumerate() {
            if i > 0 {
                out.push_str("\r\n");
            }
            out.push_str(&format!("\x1b[{s}m{body}\x1b[0m"));
        }
        out
    };
    feed(
        &mut t,
        lines(["10", "10", "1;10", "10", "10", "10"]).as_bytes(),
    );
    let (plain, _) = arms.draw(&mut t);
    feed(
        &mut t,
        lines(["10", "10", "1;10", "10", "20", "20"]).as_bytes(),
    );
    let (fraktur, fraktur_cell) = arms.draw(&mut t);
    assert!(
        differing(&plain, &fraktur) > 0,
        "the Fraktur face never reached the instance arm's pixels"
    );
    assert_eq!(
        differing(&fraktur, &fraktur_cell),
        0,
        "the GPU arms disagree on Fraktur"
    );
    feed(
        &mut t,
        lines(["11", "19", "1;11", "12", "20", "11;20"]).as_bytes(),
    );
    let (a, b) = arms.draw(&mut t);
    let cpu = cpu_frame(&mut arms, &mut t);
    assert!(
        differing(&plain, &a) > 0,
        "the alternative fonts never reached the instance arm's pixels"
    );
    assert_eq!(differing(&a, &b), 0, "the GPU arms disagree on the fonts");
    assert_eq!(differing(&a, &cpu), 0, "the CPU arm disagrees on the fonts");
    // Font 2, named no face, paints the primary's row.
    feed(
        &mut t,
        lines(["12", "12", "1;12", "12", "12", "12"]).as_bytes(),
    );
    let (unnamed, unnamed_cell) = arms.draw(&mut t);
    assert_eq!(differing(&plain, &unnamed), 0, "font 2 left the primary");
    assert_eq!(
        differing(&unnamed, &unnamed_cell),
        0,
        "the arms disagree on font 2"
    );
}

/// **`SGR 26` draws at the cell pitch on all three arms**: the text that
/// follows it "may be presented with proportional spacing at the
/// recipient's option" (`[[source: itu-t61]]`, Note 1), and baud keeps the
/// pitch, so text under 26 -- a ligature, a wide character, a bold run, an
/// underline and a frame among it, and 26 cancelled by 50 midway -- paints
/// exactly what the same text paints without it, on every arm. The arms
/// agreeing with each other is the cell oracle's half; the premise that the
/// scene drew text at all is that it differs from a blank screen.
fn proportional_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    let body = "ab->x \u{4e2d}\u{6587} \x1b[1mbold\x1b[22m \x1b[4mline\x1b[24m \
                \x1b[51mbox\x1b[54m iii WWW\r\nnext row";
    let with = format!("\x1bc\x1b[?25l\x1b[26m{body}\x1b[50m tail");
    let without = format!("\x1bc\x1b[?25l{body} tail");
    feed(t, b"\x1bc\x1b[?25l");
    let (blank, _) = arms.draw(t);
    feed(t, without.as_bytes());
    let (plain, plain_cell) = arms.draw(t);
    let plain_cpu = cpu_frame(arms, t);
    assert!(
        differing(&blank, &plain) > 0,
        "the proportional scene drew no text"
    );
    feed(t, with.as_bytes());
    let (a, b) = arms.draw(t);
    let cpu = cpu_frame(arms, t);
    assert_eq!(
        differing(&plain, &a),
        0,
        "SGR 26 moved the instance arm's pixels"
    );
    assert_eq!(
        differing(&plain_cell, &b),
        0,
        "SGR 26 moved the cell arm's pixels"
    );
    assert_eq!(
        differing(&plain_cpu, &cpu),
        0,
        "SGR 26 moved the CPU arm's pixels"
    );
    assert_eq!(differing(&a, &b), 0, "the GPU arms disagree under SGR 26");
}

/// **A box or block character two cells wide is drawn alike on all three
/// arms**: under `ambiguous_width = "wide"` (note V, V14), box lines, a
/// rounded box, diagonals, dashes, double lines, block elements and shades
/// two cells each, a heavy corner in red, a reversed line, the one-cell
/// pieces East Asian Width leaves narrow and a DEC Special Graphics frame,
/// a wide line with a mark attached (drawn by a record on the cell arm),
/// the block cursor over a wide line; then the same on a double-width line
/// and a double-height pair. The cell arm takes the two-cell mask from the
/// map entry its span bit keys, the instance and CPU arms from the
/// renderer's procedural slot, so an arm that drew the one-cell mask would
/// differ. Each wide frame against its premise: the same text under
/// "narrow" paints differently. The policy is back at "narrow" after.
fn ambiguous_box_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    use baud_domain::AmbiguousWidth;
    let scene = "\x1bc\x1b[?25l\x1b[2;1H┌──┬──┐ ▌█▀ ╭─╮ ╱╲╳ ┄┅┆ ╔═╗\
                 \x1b[3;1H│▒▓│    │ ▄▏▕ ╰─╯ ┊┋┃ ║ ║\
                 \x1b[4;1H└──┴──┘ \x1b[31m━┃┏\x1b[0m \x1b[7m─┼\x1b[0m ╚═╝\
                 \x1b[5;1H\x1b(0lqqk\x1b(B ╴─╶ ╌╍ ─╼━ ─\u{301}─\x1b[?25h\x1b[3;1H";
    let sized = "\x1bc\x1b[?25l\x1b[2;1H\x1b#6┌──┐ ▌█ ╭╮ ╱\
                 \x1b[4;1H\x1b#3┌──┐ ▀▄ ═║\x1b[5;1H\x1b#4┌──┐ ▀▄ ═║";
    // On an explicit right-to-left line, each row drawn in reverse: with
    // `?2500` a wide box line's mask is mirrored whole (`Facing`), drawn by
    // a record on the cell arm and by the mirrored slot on the others.
    // Each line a paragraph of its own, given its path and its mirroring
    // at its start.
    let rtl = |boxes: &str| {
        (2..=5).fold(scene.replace("\x1bc", "\x1bc\x1b[8l"), |text, n| {
            text.replace(
                &format!("\x1b[{n};1H"),
                &format!("\x1b[{n};1H{boxes}\x1b[2 k"),
            )
        })
    };
    let mirrored = rtl("\x1b[?2500h");
    let reversed = rtl("");
    let mut frames = Vec::new();
    let reads = arms.reads;
    for (name, text) in [
        ("single", scene.to_string()),
        ("sized", sized.to_string()),
        ("mirrored", mirrored),
        ("reversed", reversed),
    ] {
        // A row is drawn in another order from the frame's copy, as the
        // app draws it, not from the core's grid.
        arms.reads = match name {
            "mirrored" | "reversed" => Reads::Copy,
            _ => reads,
        };
        t.set_ambiguous_width(AmbiguousWidth::Narrow);
        feed(t, text.as_bytes());
        let (narrow, _) = arms.draw(t);
        t.set_ambiguous_width(AmbiguousWidth::Wide);
        feed(t, text.as_bytes());
        let (a, b) = arms.draw(t);
        let cpu = cpu_frame(arms, t);
        assert!(
            differing(&narrow, &a) > 0,
            "{name}: the policy never reached the pixels"
        );
        assert_eq!(differing(&a, &b), 0, "{name}: the GPU arms disagree");
        assert_eq!(differing(&a, &cpu), 0, "{name}: the CPU arm disagrees");
        frames.push(a);
    }
    arms.reads = reads;
    assert!(
        differing(&frames[2], &frames[3]) > 0,
        "mirroring never reached the pixels"
    );
    assert!(
        differing(&frames[0], &frames[3]) > 0,
        "the reversal never reached the pixels"
    );
    t.set_ambiguous_width(AmbiguousWidth::Narrow);
    feed(t, b"\x1bc");
}

/// **A line that is not single-width is drawn alike on all three arms**
/// (`[[source: dec-std-070]]` § 5.6.1): the cell arm masks it and draws it
/// by the records the instance arm converts it to, and its backgrounds from
/// the cell under half of each column. Each scene against its premise --
/// the renditions moved the instance arm's pixels -- so three arms that
/// all drew the lines single-width could not agree here vacuously: a
/// double-width line with a ligature and an underline, a double-height pair
/// with a coloured background, a frame of DEC line drawing on double-height
/// pairs, the cursor in each shape on a sized line, a selection across
/// one, and a sized line scrolled back into the history.
fn line_rendition_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    let base = "\x1bc\x1b[?25l";
    let lines = |sized: bool| {
        let (w, top, bottom) = match sized {
            true => ("\x1b#6", "\x1b#3", "\x1b#4"),
            false => ("", "", ""),
        };
        format!(
            "{base}\x1b[2;3H{w}Wide a -> b \x1b[4mtext\x1b[0m 0123\x1b[4;3H{top}\x1b[44;33mTall\x1b[0m line             \x1b[5;3H{bottom}\x1b[44;33mTall\x1b[0m line\x1b[7;1Hsingle below"
        )
    };
    feed(t, lines(false).as_bytes());
    let (plain, _) = arms.draw(t);
    feed(t, lines(true).as_bytes());
    let (a, b) = arms.draw(t);
    let cpu = cpu_frame(arms, t);
    assert!(
        differing(&plain, &a) > 0,
        "the renditions never reached the instance arm's pixels"
    );
    assert_eq!(differing(&a, &b), 0, "the GPU arms disagree on sized lines");
    assert_eq!(
        differing(&a, &cpu),
        0,
        "the CPU arm disagrees on sized lines"
    );
    // The frame of line drawing on double-height pairs, vttest's shape.
    let mut frame = format!("{base}\x1b(0");
    for (k, row) in [
        "lqqqqqqk", "lqqqqqqk", "x      x", "x      x", "mqqqqqqj", "mqqqqqqj",
    ]
    .iter()
    .enumerate()
    {
        let half = if k % 2 == 0 { "\x1b#3" } else { "\x1b#4" };
        frame.push_str(&format!("\x1b[{};1H{half}{row}", k + 2));
    }
    frame.push_str("\x1b(B\x1b[4;3H\x1b[1;31mhi\x1b[5;3Hhi\x1b[0m");
    feed(t, frame.as_bytes());
    let (a, b) = arms.draw(t);
    let cpu = cpu_frame(arms, t);
    assert_eq!(differing(&a, &b), 0, "the GPU arms disagree on the frame");
    assert_eq!(differing(&a, &cpu), 0, "the CPU arm disagrees on the frame");
    // The cursor in each shape, on a double-width line and on each half of a
    // double-height pair.
    for (shape, name) in [
        ("\x1b[2 q", "block"),
        ("\x1b[4 q", "underline"),
        ("\x1b[6 q", "bar"),
    ] {
        for (at, what) in [
            ("\x1b[2;5H", "double width"),
            ("\x1b[4;5H", "top half"),
            ("\x1b[5;5H", "bottom half"),
        ] {
            feed(t, format!("{}\x1b[?25h{shape}{at}", lines(true)).as_bytes());
            let (a, b) = arms.draw(t);
            let cpu = cpu_frame(arms, t);
            assert_eq!(
                differing(&a, &b),
                0,
                "the GPU arms disagree on a {name} cursor, {what}"
            );
            assert_eq!(
                differing(&a, &cpu),
                0,
                "the CPU arm disagrees on a {name} cursor, {what}"
            );
        }
    }
    // A selection from a single line into the double-width one and the pair.
    feed(t, lines(true).as_bytes());
    arms.highlight = Some(Highlight::new(
        (Row::from_index(0), Col::at_index(30)),
        (Row::from_index(4), Col::at_index(5)),
        Region::Stream,
    ));
    let (a, b) = arms.draw(t);
    let cpu = cpu_frame(arms, t);
    arms.highlight = None;
    assert_eq!(
        differing(&a, &b),
        0,
        "the GPU arms disagree on a selected sized line"
    );
    assert_eq!(
        differing(&a, &cpu),
        0,
        "the CPU arm disagrees on a selected sized line"
    );
    // An encircled run (`SGR 52`) on each rendition: the rings drawn at the
    // character's box, on all three arms.
    feed(
        t,
        format!(
            "{base}\x1b[2;3H\x1b#6\x1b[52mo\x1b[0m \x1b[52mring\x1b[0m\x1b[4;3H\x1b#3\x1b[52mab\x1b[0m\x1b[5;3H\x1b#4\x1b[52mab\x1b[0m"
        )
        .as_bytes(),
    );
    let (a, b) = arms.draw(t);
    let cpu = cpu_frame(arms, t);
    assert_eq!(differing(&a, &b), 0, "the GPU arms disagree on rings");
    assert_eq!(differing(&a, &cpu), 0, "the CPU arm disagrees on rings");
    // An input method's composition on a double-width line and on each half
    // of a double-height pair, a wide character among it.
    for at in ["\x1b[2;5H", "\x1b[4;5H", "\x1b[5;5H"] {
        feed(t, format!("{}\x1b[?25h{at}", lines(true)).as_bytes());
        let (row, col) = t.cursor();
        arms.composition = Some(Composition {
            row,
            col,
            text: "x\u{231a}y"
                .chars()
                .map(|c| baud_domain::Codepoint::new(u32::from(c)).unwrap())
                .collect(),
            measure: baud_render::Measure::default(),
            hints: Vec::new(),
            caret: Caret::End,
        });
        let (a, b) = arms.draw(t);
        let cpu = cpu_frame(arms, t);
        arms.composition = None;
        let (plain, _) = arms.draw(t);
        assert!(
            differing(&a, &plain) > 0,
            "the composition at {at:?} is drawn"
        );
        assert_eq!(
            differing(&a, &b),
            0,
            "the GPU arms disagree on a composition at {at:?}"
        );
        assert_eq!(
            differing(&a, &cpu),
            0,
            "the CPU arm disagrees on a composition at {at:?}"
        );
    }
    // **A sized line leaving the window under a smooth scroll** is drawn
    // at its size: a full-row slide with a double-width line just above
    // the window is the screen one line up, where that line is row 0 and
    // drawn by its records -- on the cell arm, and on the instance and CPU
    // arms, which draw no slide, so the three paths agree on the leaving
    // row's pixels; and mid-slide the two cell layouts agree.
    {
        let rows = usize::from(WS.ws_row);
        let mut s = format!("{base}\x1b#6Wide \x1b[52mo\x1b[0m \x1b[4mleaving\x1b[0m 0123");
        for i in 0..rows {
            s.push_str(&format!("\r\nrow {i:02} single width below"));
        }
        feed(t, s.as_bytes());
        let above = t.view(Rows::of_count(1));
        assert_eq!(
            above.rendition(Row::ZERO),
            baud_domain::LineRendition::DoubleWidth,
            "the premise: the line above the window is double-width"
        );
        let cell_h = arms.renderer.metrics().cell_h;
        arms.back = Rows::ZERO;
        arms.slide = Some(cell_h);
        let (_, full_slid) = arms.draw(t);
        arms.slide = None;
        arms.back = Rows::of_count(1);
        let (instance_up, one_up) = arms.draw(t);
        let cpu_up = cpu_frame(arms, t);
        arms.back = Rows::ZERO;
        for (arm, up) in [
            ("cell", &one_up),
            ("instance", &instance_up),
            ("cpu", &cpu_up),
        ] {
            assert_eq!(
                differing(&full_slid, up),
                0,
                "a full-row slide off a double-width line is not the {arm} arm's screen one line up"
            );
        }
        arms.slide = Some(Px::of(cell_h.value() / 2));
        let (_, mid) = arms.draw(t);
        let mid_copy = arms.draw_copied(t);
        arms.slide = None;
        assert_eq!(
            differing(&mid, &mid_copy),
            0,
            "the two cell layouts disagree mid-slide off a double-width line"
        );
        // One line more: the leaving row is single-width again, and draws
        // from the map, the sized row's mask gone with it.
        feed(t, b"\r\nrow single width last");
        assert!(!t.view(Rows::of_count(1)).rendition(Row::ZERO).is_double());
        arms.slide = Some(cell_h);
        let (_, full_slid) = arms.draw(t);
        arms.slide = None;
        arms.back = Rows::of_count(1);
        let (instance_up, one_up) = arms.draw(t);
        let cpu_up = cpu_frame(arms, t);
        arms.back = Rows::ZERO;
        for (arm, up) in [
            ("cell", &one_up),
            ("instance", &instance_up),
            ("cpu", &cpu_up),
        ] {
            assert_eq!(
                differing(&full_slid, up),
                0,
                "a full-row slide off the single-width line after it is not the {arm} arm's screen one line up"
            );
        }
    }
    // **A sized line's records reused across frames** stay what a fresh
    // conversion draws: frame after frame on one renderer, with only
    // another row changing, then a palette entry the line uses redefined,
    // the cursor moving onto the line and along it, a blink phase, the
    // cluster store and the selection's colours, the arms agree at each.
    feed(
        t,
        format!(
            "{base}\x1b[2;3H\x1b#6\x1b[31mred\x1b[0m \x1b[5mblink\x1b[0m e\u{301}\x1b[6;1Hother"
        )
        .as_bytes(),
    );
    for (step, bytes) in [
        ("as it was", ""),
        ("another row", "\x1b[6;1Hchanged"),
        ("the palette", "\x1b]4;1;rgb:00/c0/40\x1b\\"),
        ("the cursor onto it", "\x1b[?25h\x1b[2;2H"),
        ("the cursor along it", "\x1b[2;4H"),
        ("the cursor's shape", "\x1b[4 q"),
        ("the cursor colour", "\x1b]12;rgb:ff/00/ff\x1b\\"),
        ("the cursor hidden", "\x1b[?25l"),
        ("a cell of the line, its glyph drawn already", "\x1b[2;4Hr"),
    ] {
        feed(t, bytes.as_bytes());
        let (a, b) = arms.draw(t);
        let cpu = cpu_frame(arms, t);
        assert_eq!(differing(&a, &b), 0, "the GPU arms disagree after {step}");
        assert_eq!(differing(&a, &cpu), 0, "the CPU arm disagrees after {step}");
    }
    arms.renderer.set_blink(BlinkPhases {
        slow: Blink::Hidden,
        rapid: Blink::Shown,
    });
    let (a, b) = arms.draw(t);
    arms.renderer.set_blink(BlinkPhases::SHOWN);
    assert_eq!(
        differing(&a, &b),
        0,
        "the GPU arms disagree in the blink's off phase"
    );
    let (a, b) = arms.draw(t);
    assert_eq!(
        differing(&a, &b),
        0,
        "the GPU arms disagree back in the on phase"
    );
    // A selection over the line while its records are kept, and taken
    // off again, nothing else changing.
    let (a, b) = arms.draw(t);
    assert_eq!(
        differing(&a, &b),
        0,
        "the GPU arms disagree before the selection"
    );
    for (step, region) in [
        (
            "a selection over the line",
            Some(Highlight::new(
                (Row::from_index(1), Col::at_index(0)),
                (Row::from_index(1), Col::at_index(6)),
                Region::Stream,
            )),
        ),
        ("the selection taken off", None),
    ] {
        arms.highlight = region;
        let (a, b) = arms.draw(t);
        let cpu = cpu_frame(arms, t);
        assert_eq!(differing(&a, &b), 0, "the GPU arms disagree after {step}");
        assert_eq!(differing(&a, &cpu), 0, "the CPU arm disagrees after {step}");
    }
    feed(t, b"\x1b]112\x1b\\\x1b[0 q");
    // **What moves a sized line's records, and what does not.** A glyph
    // placed in the atlas for another row leaves them (a placement takes a
    // free place and moves no slot they name); a reset of the atlas, which
    // voids every slot, reconverts the line; and under the picture finder a
    // steady frame reuses them while a block that reaches the line through
    // another row reconverts it. The arms agree at each.
    let converted = |arms: &Arms<'_, '_>| arms.renderer.sized_converted().value();
    // The map holds a screen's worth of glyphs again after the reset.
    let abc = "abcdefghijklmnopqrstuvwxyz ABCDEFGHIJKLMNOPQRSTUVWXYZ 0123456789";
    feed(
        t,
        format!("{base}\x1b[2;3H\x1b#6\x1b[31mred\x1b[0m line\x1b[6;1Hother\x1b[8;1H{abc}")
            .as_bytes(),
    );
    let _ = arms.draw(t);
    let (before, drawn) = (converted(arms), arms.renderer.atlas_stats().rasterised);
    feed(t, "\x1b[6;1H\u{20ac}\u{151}\u{3a9}\u{416}".as_bytes());
    let (a, b) = arms.draw(t);
    assert!(
        arms.renderer.atlas_stats().rasterised.value() > drawn.value(),
        "the premise: the other row's glyphs were placed"
    );
    assert_eq!(
        converted(arms),
        before,
        "a glyph placed for another row reconverted the line"
    );
    assert_eq!(
        differing(&a, &b),
        0,
        "the GPU arms disagree after a glyph elsewhere"
    );
    for (emoji, step) in [
        (baud_render::EmojiDefault::System, "an atlas reset"),
        (baud_render::EmojiDefault::Font, "an atlas reset back"),
    ] {
        let before = converted(arms);
        arms.renderer.set_emoji_default(emoji);
        let (a, b) = arms.draw(t);
        assert!(converted(arms) > before, "{step} kept the line's records");
        assert_eq!(differing(&a, &b), 0, "the GPU arms disagree after {step}");
    }
    // **A zoom, and a zoom back to a size the atlas holds.** The zoom back
    // draws no glyph again and replaces neither sheet, so both GPU arms draw
    // from the textures they hold, through a map and records rebuilt for
    // the size; the arms agree with each other and with the CPU arm at
    // every step. These steps compare the arms with each other only, never
    // with a fresh draw: were the glyph memo carried to a new size, every
    // arm would still agree. The fresh-draw guard is
    // `baud-render/tests/zoom.rs`.
    let drawn = |arms: &Arms<'_, '_>| {
        let s = arms.renderer.atlas_stats();
        s.rasterised.value() + s.coloured.value() + s.synthesised.value()
    };
    for (px, step, again) in [
        (20.0, "a zoom", false),
        (14.0, "a zoom back", true),
        (20.0, "a zoom in again", true),
        (14.0, "the zoom back to the start", true),
    ] {
        let (before, glyphs) = (converted(arms), drawn(arms));
        arms.renderer
            .rescale(PxSize::of(px).unwrap())
            .expect("a size the face takes");
        let (a, b) = arms.draw(t);
        let cpu = cpu_frame(arms, t);
        assert_eq!(differing(&a, &b), 0, "the GPU arms disagree after {step}");
        assert_eq!(differing(&a, &cpu), 0, "the CPU arm disagrees after {step}");
        assert!(converted(arms) > before, "{step} kept the line's records");
        match again {
            true => assert_eq!(drawn(arms), glyphs, "{step} drew glyphs again"),
            false => assert!(drawn(arms) > glyphs, "{step} draws the line's glyphs"),
        }
    }
    let resting = arms.renderer.theme().clone();
    arms.renderer.set_theme(Theme {
        app_bg: baud_render::AppBg::Dimmed,
        picture_bg: baud_render::PictureBg::Opaque,
        ..resting.clone()
    });
    // Ten colours on the line and ten more on the row below, apart from
    // it: a picture only once the row below joins the block
    // (`PICTURE_COLOURS`), which a DCH there does without a rendition
    // minted or a cell of the line changed.
    let colours = |row: usize, col: usize, from: u32| {
        let mut b = format!("\x1b[{row};{col}H");
        for k in 0..10u32 {
            let c = from + k * 9;
            b.push_str(&format!("\x1b[38;2;250;250;250;48;2;{c};{};90mx", 255 - c));
        }
        b.push_str("\x1b[0m");
        b
    };
    feed(
        t,
        format!(
            "{base}\x1b[2;1H\x1b#6{}{}\x1b[8;1H{abc}",
            colours(2, 3, 10),
            colours(3, 41, 100)
        )
        .as_bytes(),
    );
    let (a, b) = arms.draw(t);
    assert_eq!(
        arms.exceptions.stats().pictured.value(),
        0,
        "ten colours make no picture"
    );
    assert_eq!(
        differing(&a, &b),
        0,
        "the GPU arms disagree before the picture"
    );
    let before = converted(arms);
    let (a, b) = arms.draw(t);
    assert_eq!(
        converted(arms),
        before,
        "a steady frame under the finder reconverted the line"
    );
    assert_eq!(
        differing(&a, &b),
        0,
        "the GPU arms disagree on the steady frame"
    );
    feed(t, b"\x1b[3;1H\x1b[38P");
    let (a, b) = arms.draw(t);
    assert!(
        arms.exceptions.stats().pictured.value() >= 20,
        "the premise: the row below made the line a picture"
    );
    assert!(
        converted(arms) > before,
        "the picture reaching the line kept its records"
    );
    assert_eq!(
        differing(&a, &b),
        0,
        "the GPU arms disagree once the line is a picture"
    );
    let finder = arms.renderer.theme().clone();
    // vttest's double-size test from its own bytes, a frame every 128
    // bytes on one renderer, with the picture finder off and on: the cell
    // arm's reused lines are the instance arm's conversion at every frame.
    let vttest = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/vttest/doublesize.raw"
    ))
    .expect("the recording");
    for (theme, name) in [(resting.clone(), "off"), (finder, "on")] {
        arms.renderer.set_theme(theme);
        feed(t, b"\x1bc");
        for (k, piece) in vttest.chunks(128).enumerate() {
            feed(t, piece);
            let (a, b) = arms.draw(t);
            assert_eq!(
                differing(&a, &b),
                0,
                "the GPU arms disagree on vttest's double-size test at read {k}, the finder {name}"
            );
        }
    }
    arms.renderer.set_theme(resting);
    // A sized line scrolled back into the history and drawn there.
    let rows = usize::from(WS.ws_row);
    feed(t, format!("{}\x1b[{rows};1H\n\n\n", lines(true)).as_bytes());
    arms.back = Rows::of_count(3);
    let (a, b) = arms.draw(t);
    let cpu = cpu_frame(arms, t);
    arms.back = Rows::ZERO;
    assert_eq!(
        differing(&a, &b),
        0,
        "the GPU arms disagree on a sized line looked back at"
    );
    assert_eq!(
        differing(&a, &cpu),
        0,
        "the CPU arm disagrees on a sized line looked back at"
    );
}

/// **Two glyphs whose ink meets are drawn in the order the instance arm
/// draws them, on all three arms**: rows top to bottom and each row left
/// to right, so the later lies over the earlier where they cross. The cell
/// arm draws most glyphs from its map and a cell the map cannot draw --
/// a mark, a decomposed letter, a ligature -- by a record after them, so a
/// map glyph to the right of a record, or in the row below it, is drawn
/// whole by a record too, and the records go in the cells' order. Each
/// scene's premise: drawn alone, the two glyphs' ink meets.
fn overlap_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    let scene = |s: &str| format!("\x1bc\x1b[?25l\x1b[3;5H{s}\x1b[0m");
    feed(t, scene("").as_bytes());
    let (empty, _) = arms.draw(t);
    let mut split = Vec::new();
    // (name, scene, the first glyph alone, the second alone).
    for (name, sc, first, second) in [
        (
            "a record and a glyph to its right",
            "\x1b[31;3mW\u{301}\x1b[0;32mW",
            "\x1b[31;3mW\u{301}\x1b[0;32m ",
            "\x1b[31;3m \x1b[0;32mW",
        ),
        (
            "a record and a run to its right, each glyph meeting the next",
            "\x1b[31;3mW\u{301}\x1b[32mW\x1b[34mW",
            "\x1b[31;3mW\u{301}\x1b[32mW\x1b[34m ",
            "\x1b[31;3m \x1b[32m \x1b[34mW",
        ),
        (
            "a decomposed letter and a glyph to its right",
            "\x1b[31m\u{1fa}\x1b[0;32;3m\u{c5}",
            "\x1b[31m\u{1fa}\x1b[0;32;3m ",
            "\x1b[31m \x1b[0;32;3m\u{c5}",
        ),
        (
            "a record and a glyph in the row below",
            "\x1b[31mg\u{323}\u{323}\u{323}\x1b[4;5H\x1b[32m\u{c5}",
            "\x1b[31mg\u{323}\u{323}\u{323}",
            "\x1b[4;5H\x1b[32m\u{c5}",
        ),
        (
            "a record and a ligature to its right",
            "\x1b[31mW\u{301}\x1b[32;3m&&",
            "\x1b[31mW\u{301}\x1b[32;3m  ",
            "\x1b[31m \x1b[32;3m&&",
        ),
        (
            "a ligature and a glyph to its right",
            "\x1b[31;3m&&\x1b[0;32mW",
            "\x1b[31;3m&&\x1b[0;32m ",
            "\x1b[31;3m  \x1b[0;32mW",
        ),
    ] {
        feed(t, scene(first).as_bytes());
        let (one, _) = arms.draw(t);
        feed(t, scene(second).as_bytes());
        let (two, _) = arms.draw(t);
        let meet = (0..empty.len())
            .filter(|&i| one[i] != empty[i] && two[i] != empty[i])
            .count();
        feed(t, scene(sc).as_bytes());
        let (a, b) = arms.draw(t);
        let cpu = cpu_frame(arms, t);
        assert_ne!(meet, 0, "{name}: the premise, the two glyphs' ink meets");
        let (gpu, soft) = (differing(&a, &b), differing(&a, &cpu));
        if gpu + soft != 0 {
            split.push(format!("{name}: cell arm {gpu} px, CPU arm {soft} px"));
        }
    }
    assert!(
        split.is_empty(),
        "the arms draw two glyphs that meet in different orders: {split:#?}"
    );
}

/// **A glyph that reaches out of its cell takes one order against the
/// decorations it reaches, on all three arms**: each row's glyphs and then
/// that row's decorations, rows top to bottom, as the instance arm draws
/// them and as CSS paints line boxes (`[[source: css21-zindex]]`,
/// E.2, 7.2.1). So a glyph beside a decorated cell, a record's (a
/// cluster with marks) as much as the map's, lies under that cell's
/// ring, frame, overline and strike, and under its own underline; a
/// glyph reaching up from the row below lies over them; one reaching
/// down from the row above lies under them. Each scene carries two
/// premises -- the reaching ink lands in the decorated cell, and the
/// decoration is drawn there -- so three arms that all dropped either
/// could not agree here vacuously.
fn reach_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    let (pw, _) = arms.renderer.frame_size();
    let w = usize::try_from(pw.value()).unwrap();
    let m = arms.renderer.metrics();
    let (cw, ch) = (
        usize::try_from(m.cell_w.value()).unwrap(),
        usize::try_from(m.cell_h.value()).unwrap(),
    );
    // How many pixels of cell `(col, row)` differ between two frames.
    let in_cell = |a: &[u32], b: &[u32], (col, row): (usize, usize)| {
        (row * ch..(row + 1) * ch)
            .flat_map(|y| (col * cw..(col + 1) * cw).map(move |x| y * w + x))
            .filter(|&i| a[i] != b[i])
            .count()
    };
    let scene = |s: &str| format!("\x1bc\x1b[?25l{s}\x1b[0m");
    let mut split = Vec::new();
    // (name, scene, the scene without the decoration, that one without the
    // reaching glyph's ink as well, the decorated cell). A decorated cell
    // beside a reaching glyph holds a space, so no glyph's ink meets
    // another's there: the order of two glyphs is not this gate's.
    for (name, sc, bare, plain, cell) in [
        (
            "a record beside a ring",
            "\x1b[3;5H\x1b[31;3mW\u{301}\x1b[0;32;52m ",
            "\x1b[3;5H\x1b[31;3mW\u{301}\x1b[0;32m ",
            "\x1b[3;5H\x1b[31;3m \x1b[0;32m ",
            (5, 2),
        ),
        (
            "a record beside a frame",
            "\x1b[3;5H\x1b[31;3mW\u{301}\x1b[0;32;51m ",
            "\x1b[3;5H\x1b[31;3mW\u{301}\x1b[0;32m ",
            "\x1b[3;5H\x1b[31;3m \x1b[0;32m ",
            (5, 2),
        ),
        (
            "a record beside an overline",
            "\x1b[3;5H\x1b[31;3mW\u{301}\x1b[0;32;53m ",
            "\x1b[3;5H\x1b[31;3mW\u{301}\x1b[0;32m ",
            "\x1b[3;5H\x1b[31;3m \x1b[0;32m ",
            (5, 2),
        ),
        (
            "a record beside a strike",
            "\x1b[3;5H\x1b[31;3mW\u{301}\x1b[0;32;9m ",
            "\x1b[3;5H\x1b[31;3mW\u{301}\x1b[0;32m ",
            "\x1b[3;5H\x1b[31;3m \x1b[0;32m ",
            (5, 2),
        ),
        (
            "a record under its own underline",
            "\x1b[3;5H\x1b[31;58;5;33;4mg\u{301}",
            "\x1b[3;5H\x1b[31mg\u{301}",
            "\x1b[3;5H\x1b[31m \u{301}",
            (4, 2),
        ),
        (
            "a glyph from the row below into a ring",
            "\x1b[3;5H\x1b[32;52mx\x1b[4;5H\x1b[0;31m\u{1d8}",
            "\x1b[3;5H\x1b[32mx\x1b[4;5H\x1b[0;31m\u{1d8}",
            "\x1b[3;5H\x1b[32mx\x1b[4;5H\x1b[0;31mu",
            (4, 2),
        ),
        (
            "a glyph from the row below into a double underline",
            "\x1b[3;5H\x1b[32;21mx\x1b[4;5H\x1b[0;31m\u{1d8}",
            "\x1b[3;5H\x1b[32mx\x1b[4;5H\x1b[0;31m\u{1d8}",
            "\x1b[3;5H\x1b[32mx\x1b[4;5H\x1b[0;31mu",
            (4, 2),
        ),
        (
            "a glyph from the row below into a frame",
            "\x1b[3;5H\x1b[32;51mx\x1b[4;5H\x1b[0;31m\u{1d8}",
            "\x1b[3;5H\x1b[32mx\x1b[4;5H\x1b[0;31m\u{1d8}",
            "\x1b[3;5H\x1b[32mx\x1b[4;5H\x1b[0;31mu",
            (4, 2),
        ),
        (
            "a record from the row below into a ring",
            "\x1b[3;5H\x1b[32;52mx\x1b[4;5H\x1b[0;31mA\u{301}\u{301}",
            "\x1b[3;5H\x1b[32mx\x1b[4;5H\x1b[0;31mA\u{301}\u{301}",
            "\x1b[3;5H\x1b[32mx\x1b[4;5H\x1b[0;31mA",
            (4, 2),
        ),
    ] {
        feed(t, scene(plain).as_bytes());
        let (none, _) = arms.draw(t);
        feed(t, scene(bare).as_bytes());
        let (no_dec, _) = arms.draw(t);
        feed(t, scene(sc).as_bytes());
        let (a, b) = arms.draw(t);
        let cpu = cpu_frame(arms, t);
        assert_ne!(
            in_cell(&no_dec, &none, cell),
            0,
            "{name}: the premise, the glyph's ink reaches the decorated cell"
        );
        assert_ne!(
            in_cell(&a, &no_dec, cell),
            0,
            "{name}: the premise, the decoration is drawn there"
        );
        let (gpu, soft) = (differing(&a, &b), differing(&a, &cpu));
        if gpu + soft != 0 {
            split.push(format!("{name}: cell arm {gpu} px, CPU arm {soft} px"));
        }
    }
    assert!(
        split.is_empty(),
        "the arms put a reaching glyph on different sides of a decoration: {split:#?}"
    );
}

/// **A composition lies over the decorations of the text under it, the
/// ring as the underline, on all three arms.** The instance and CPU paths
/// blank the cells it covers in their own renditions and draw its records
/// after every row, so the decorations of those cells -- an underline, a
/// strike, a frame, an encircled run's outline -- stay, under its
/// glyphs, and its own underline lies over theirs; the cell
/// path draws them in its decoration pass and the composition's records
/// after it. The composition covers the run's last two cells, its outline's
/// right end among them. Each scene's premise: the decoration under the
/// composition reaches the pixels of those cells on both GPU arms.
fn composed_decoration_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    let (pw, _) = arms.renderer.frame_size();
    let w = usize::try_from(pw.value()).unwrap();
    let m = arms.renderer.metrics();
    let (cw, ch) = (
        usize::try_from(m.cell_w.value()).unwrap(),
        usize::try_from(m.cell_h.value()).unwrap(),
    );
    // How many pixels of the composition's cells, columns 6 and 7 of row
    // 2, differ between two frames.
    let under = |a: &[u32], b: &[u32]| {
        (2 * ch..3 * ch)
            .flat_map(|y| (6 * cw..8 * cw).map(move |x| y * w + x))
            .filter(|&i| a[i] != b[i])
            .count()
    };
    let scene = |s: &str| format!("\x1bc\x1b[?25l\x1b[3;3H\x1b[32{s}mabcdef\x1b[3;7H");
    let mut split = Vec::new();
    for (name, sgr) in [
        ("encircled", ";52"),
        ("double underlined", ";21"),
        ("struck", ";9"),
        ("framed", ";51"),
    ] {
        arms.composition = Some(Composition {
            row: Row::from_index(2),
            col: Col::at_index(6),
            text: "Wg"
                .chars()
                .map(|c| baud_domain::Codepoint::new(u32::from(c)).unwrap())
                .collect(),
            measure: baud_render::Measure::default(),
            hints: Vec::new(),
            caret: Caret::End,
        });
        feed(t, scene("").as_bytes());
        let (bare_a, bare_b) = arms.draw(t);
        feed(t, scene(sgr).as_bytes());
        let (a, b) = arms.draw(t);
        let cpu = cpu_frame(arms, t);
        arms.composition = None;
        assert_ne!(
            under(&a, &bare_a),
            0,
            "{name}: the premise, the decoration is drawn on the instance arm"
        );
        assert_ne!(
            under(&b, &bare_b),
            0,
            "{name}: the premise, the decoration is drawn on the cell arm"
        );
        let (gpu, soft) = (differing(&a, &b), differing(&a, &cpu));
        if gpu + soft != 0 {
            split.push(format!("{name}: cell arm {gpu} px, CPU arm {soft} px"));
        }
    }
    assert!(
        split.is_empty(),
        "the arms disagree on the decorations under a composition: {split:#?}"
    );
}

/// **A framed run closes beside a framed cell whose text is hidden**, on
/// each arm against itself: a concealed cell, or one in its blink's off
/// phase, draws its background and nothing of its own, so the visible
/// cells draw a side at the seam -- the frame of the same text with the
/// hidden cells never framed. Both arms shared the open seam, so their
/// agreeing could not show it; each arm is held to its own reference.
fn hidden_seam_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    let off = BlinkPhases {
        slow: Blink::Hidden,
        rapid: Blink::Hidden,
    };
    let shown = BlinkPhases::SHOWN;
    let mut open_at = Vec::new();
    for (seam, closed, blink, name) in [
        (
            "\x1b[51mab\x1b[8mcd",
            "\x1b[51mab\x1b[0;8mcd",
            shown,
            "concealed after",
        ),
        (
            "\x1b[51;8mab\x1b[28mcd",
            "\x1b[8mab\x1b[0;51mcd",
            shown,
            "concealed before",
        ),
        (
            "\x1b[51mab\x1b[5mcd",
            "\x1b[51mab\x1b[0;5mcd",
            off,
            "slow blink off",
        ),
        (
            "\x1b[51mab\x1b[6mcd",
            "\x1b[51mab\x1b[0;6mcd",
            off,
            "rapid blink off",
        ),
        (
            "\x1b[52mab\x1b[8mcd",
            "\x1b[52mab\x1b[0;8mcd",
            shown,
            "encircled, concealed",
        ),
        (
            "\x1b[52mab\x1b[5mcd",
            "\x1b[52mab\x1b[0;5mcd",
            off,
            "encircled, blink off",
        ),
    ] {
        arms.renderer.set_blink(blink);
        let scene = |s: &str| format!("\x1bc\x1b[?25l\x1b[2;3H{s}\x1b[0m");
        feed(t, scene(closed).as_bytes());
        let (want_a, want_b) = arms.draw(t);
        let whole = if seam.starts_with("\x1b[52") {
            "\x1b[52mabcd"
        } else {
            "\x1b[51mabcd"
        };
        feed(t, scene(whole).as_bytes());
        let (open_a, _) = arms.draw(t);
        assert_ne!(
            differing(&want_a, &open_a),
            0,
            "{name}: the premise, the hidden cells hide something"
        );
        let bare = closed
            .replace("\x1b[51m", "")
            .replace("\x1b[52m", "")
            .replace(";51m", "m");
        feed(t, scene(&bare).as_bytes());
        let (bare_a, bare_b) = arms.draw(t);
        assert_ne!(
            differing(&want_a, &bare_a),
            0,
            "{name}: the premise, the outline is drawn on the instance arm"
        );
        assert_ne!(
            differing(&want_b, &bare_b),
            0,
            "{name}: the premise, the outline is drawn on the cell arm"
        );
        feed(t, scene(seam).as_bytes());
        let (a, b) = arms.draw(t);
        if differing(&a, &want_a) != 0 {
            open_at.push(format!("instance arm, {name}"));
        }
        if differing(&b, &want_b) != 0 {
            open_at.push(format!("cell arm, {name}"));
        }
    }
    arms.renderer.set_blink(shown);
    assert!(
        open_at.is_empty(),
        "the box is open at the seam beside hidden cells: {open_at:?}"
    );
}

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

/// **A Sixel's background fill, on all three paths**: `P2` 0 sets the
/// positions its data leaves at zero to the background, over the raster
/// extent `Ph` by `Pv` (`[[source: vt3xx-sixel]]`), 40 by 40 over data 20
/// by 24 here, in the default background unreversed, opaque, over the text
/// under it -- the instance arm, the cell arm and the CPU arm alike, and
/// under `?5` the same colour while a blank cell shows the reversed
/// screen, and after `OSC 11` the colour it names, the next frame, with
/// one snapshot of the image kept. The control is the same Sixel with `P2`
/// 1, whose zero positions show the text. On the see-through window the
/// fill is not drawn, as before it was built, an open question: the two
/// Sixels draw alike there. And an embedder's image whose every word is
/// the fill word, with no fill asked for, draws those words as they are:
/// black at an alpha of one over the frame.
fn sixel_fill_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    let base = arms.renderer.theme().clone();
    let fw = usize::try_from(arms.renderer.frame_size().0.value()).unwrap();
    let opaque = Theme {
        bg_alpha: Chan::from_u8(0xFF),
        ..base.clone()
    };
    arms.renderer.set_theme(opaque.clone());
    let text: &[u8] = b"\x1bc\x1b[?25l\x1b[2;1HEEEEEEEEEEEEEEEEEEEE\x1b[3;1HEEEEEEEEEEEEEEEEEEEE\
        \x1b[4;1HEEEEEEEEEEEEEEEEEEEE\x1b[5;1HEEEEEEEEEEEEEEEEEEEE\x1b[2;3H";
    let sixel =
        |p2: u8| format!("\x1bP9;{p2};0q\"1;1;40;40#1;2;100;0;0#1!20i-!20i-!20i-!20A\x1b\\");
    let bg = 0xFF00_0000 | (opaque.bg.0 & 0x00FF_FFFF);
    let red = 0xFFFF_0000u32;
    let frames = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>| {
        let (a, b) = arms.draw(t);
        let c = cpu_frame(arms, t);
        let places = arms.renderer.image_placements();
        assert_eq!(places.len(), 1, "one image on screen");
        let at = (
            usize::try_from(places[0].x).unwrap(),
            usize::try_from(places[0].y).unwrap(),
        );
        ([a, b, c], at, places[0].ground)
    };
    // The region past the data, inside the extent: columns 20-39, rows 0-39.
    let past_data = |f: &[u32], (x0, y0): (usize, usize)| {
        (0..40)
            .flat_map(|y| (20..40).map(move |x| (y0 + y) * fw + x0 + x))
            .filter(|&i| f[i] != bg)
            .count()
    };
    for (what, pre) in [("normal", ""), ("reverse video", "\x1b[?5h")] {
        feed(t, text);
        feed(t, pre.as_bytes());
        feed(t, sixel(1).as_bytes());
        let (control, at, ground) = frames(arms, t);
        assert_eq!(ground, Pixel::CLEAR, "{what}: P2 1 has no fill");
        assert!(
            past_data(&control[0], at) > 0,
            "{what}: the control, P2 1 shows what is under it"
        );
        feed(t, text);
        feed(t, pre.as_bytes());
        feed(t, sixel(0).as_bytes());
        let (got, at, ground) = frames(arms, t);
        assert_eq!(
            ground.bits(),
            bg,
            "{what}: the fill is the default background"
        );
        let (x0, y0) = at;
        let px = |f: &[u32], x: usize, y: usize| f[(y0 + y) * fw + x0 + x];
        for (arm, f) in ["instance", "cell", "cpu"].iter().zip(&got) {
            assert_eq!(px(f, 0, 1), red, "{what}, {arm}: a set position");
            assert_eq!(
                px(f, 0, 0),
                bg,
                "{what}, {arm}: a zero position of the data"
            );
            assert_eq!(px(f, 39, 39), bg, "{what}, {arm}: the extent's last pixel");
            assert_eq!(
                past_data(f, at),
                0,
                "{what}, {arm}: the text under it is hidden"
            );
        }
        // Row 8, column 30: a blank cell, reversed under `?5`.
        let m = arms.renderer.metrics();
        let (cw, ch) = (
            usize::try_from(m.cell_w.value()).unwrap(),
            usize::try_from(m.cell_h.value()).unwrap(),
        );
        let blank = got[0][(8 * ch + ch / 2) * fw + 30 * cw + cw / 2];
        match pre.is_empty() {
            true => assert_eq!(blank, bg, "the blank cell is the default background"),
            false => assert_ne!(blank, bg, "the premise: ?5 reverses the blank cell"),
        }
    }
    // Read each frame: `OSC 11` moves the default background, and the next
    // frame draws the fill in it on every arm, with one snapshot kept for
    // the image whatever colours it was drawn in.
    feed(t, text);
    feed(t, sixel(0).as_bytes());
    for (osc, want) in [
        (&b"\x1b]11;rgb:10/20/30\x1b\\"[..], 0xFF10_2030u32),
        (b"\x1b]11;rgb:a0/b0/c0\x1b\\", 0xFFA0_B0C0),
    ] {
        feed(t, osc);
        let (got, (x0, y0), ground) = frames(arms, t);
        assert_eq!(ground.bits(), want, "the fill follows OSC 11");
        for (arm, f) in ["instance", "cell", "cpu"].iter().zip(&got) {
            assert_eq!(f[(y0 + 39) * fw + x0 + 39], want, "{arm}: {want:08x}");
        }
        assert_eq!(arms.renderer.image_uploads().len(), 1, "one snapshot");
    }
    feed(t, b"\x1b]111\x1b\\");
    sixel_scroll_fill(arms, t, fw);
    // **The see-through window, as `[glass] sixel_backgrounds` says**:
    // opaque, the default, as on an opaque window; dim, the background at
    // the window's dim over the text, which shows through it; clear, in
    // its place, the erased cell's pixel. All three arms agree on each,
    // and the data stays red.
    let dim = Chan::from_u8(56);
    let mut seen = Vec::new();
    for mode in [SixelBg::Opaque, SixelBg::Dimmed, SixelBg::Cleared] {
        arms.renderer.set_theme(Theme {
            bg_alpha: dim,
            sixel_bg: mode,
            ..base.clone()
        });
        feed(t, text);
        feed(t, sixel(0).as_bytes());
        let (got, at, ground) = frames(arms, t);
        let what = format!("glass, {mode:?}");
        let want = match mode {
            SixelBg::Opaque => bg,
            _ => (u32::from(dim.get()) << 24) | (opaque.bg.0 & 0x00FF_FFFF),
        };
        assert_eq!(ground.bits(), want, "{what}: the fill's word");
        assert_eq!(
            differing(&got[0], &got[1]),
            0,
            "{what}: the cell arm differs"
        );
        let cpu = channel_diff(&got[2], &got[0], |_| true);
        assert_eq!(cpu.0, 0, "{what}: the CPU arm differs by up to {}", cpu.1);
        let (x0, y0) = at;
        let px = |f: &[u32], x: usize, y: usize| f[(y0 + y) * fw + x0 + x];
        // The extent past the data, columns 20-39, rows 0-39.
        let region: Vec<u32> = (0..40)
            .flat_map(|y| (20..40).map(move |x| (x, y)))
            .map(|(x, y)| px(&got[0], x, y))
            .collect();
        let m = arms.renderer.metrics();
        let (cw, ch) = (
            usize::try_from(m.cell_w.value()).unwrap(),
            usize::try_from(m.cell_h.value()).unwrap(),
        );
        // Row 8, column 30: a blank cell, the glass's erased pixel.
        let blank = got[0][(8 * ch + ch / 2) * fw + 30 * cw + cw / 2];
        for (arm, f) in ["instance", "cell", "cpu"].iter().zip(&got) {
            assert_eq!(px(f, 0, 1), red, "{what}, {arm}: a set position");
        }
        match mode {
            SixelBg::Opaque => assert!(region.iter().all(|&p| p == bg), "{what}: solid"),
            SixelBg::Cleared => {
                assert!(
                    region.iter().all(|&p| p == blank),
                    "{what}: the erased pixel"
                );
            }
            SixelBg::Dimmed => {
                assert!(region.iter().all(|&p| p != blank), "{what}: dimmed over");
                let mut distinct = region.clone();
                distinct.sort_unstable();
                distinct.dedup();
                assert!(distinct.len() > 1, "{what}: the text shows through");
            }
        }
        seen.push(region);
        // Data past a raster extent narrower than it: its zero positions
        // there are not filled, and stay clear -- the cell under them
        // shows, on every arm, and a replacing draw does not write them.
        feed(t, text);
        feed(t, b"\x1bP9;0;0q\"1;1;10;12#1;2;100;0;0#1!20i\x1b\\");
        let (got, (x0, y0), _) = frames(arms, t);
        assert_eq!(
            differing(&got[0], &got[1]),
            0,
            "{what}, narrow: the cell arm differs"
        );
        let cpu = channel_diff(&got[2], &got[0], |_| true);
        assert_eq!(
            cpu.0, 0,
            "{what}, narrow: the CPU arm differs by up to {}",
            cpu.1
        );
        let clear = got[0][y0 * fw + x0 + 15];
        assert_ne!(clear, 0, "{what}, narrow: a clear position shows the cell");
        assert_eq!(
            got[0][(y0 + 1) * fw + x0 + 15],
            red,
            "{what}, narrow: the data"
        );
    }
    assert_ne!(seen[1], seen[2], "glass: dim is not clear");
    // Under `?5` the fill at the window's dim keeps the colour the opaque
    // fill keeps, the background unreversed.
    for mode in [SixelBg::Dimmed, SixelBg::Cleared] {
        arms.renderer.set_theme(Theme {
            bg_alpha: dim,
            sixel_bg: mode,
            ..base.clone()
        });
        feed(t, text);
        feed(t, b"\x1b[?5h");
        feed(t, sixel(0).as_bytes());
        let (got, _, ground) = frames(arms, t);
        let want = (u32::from(dim.get()) << 24) | (opaque.bg.0 & 0x00FF_FFFF);
        assert_eq!(ground.bits(), want, "glass, {mode:?}, ?5: unreversed");
        assert_eq!(
            differing(&got[0], &got[1]),
            0,
            "glass, {mode:?}, ?5: the cell arm"
        );
        let cpu = channel_diff(&got[2], &got[0], |_| true);
        assert_eq!(
            cpu.0, 0,
            "glass, {mode:?}, ?5: the CPU arm by up to {}",
            cpu.1
        );
    }
    arms.renderer.set_theme(Theme {
        bg_alpha: dim,
        ..base.clone()
    });
    // An embedder's image of fill words, no fill asked: drawn as they are.
    arms.renderer.set_theme(opaque);
    feed(t, text);
    let (under, _) = arms.draw(t);
    let (w, h) = (8usize, 8usize);
    let placed = t.put_image(ImagePx::of(8), ImagePx::of(8), ImageAlpha::Partial, |px| {
        px.fill(Pixel::FILL)
    });
    assert!(!placed.is_none(), "the embedder's image placed");
    let (got, (x0, y0), ground) = frames(arms, t);
    assert_eq!(ground, Pixel::CLEAR, "an embedder's image has no fill");
    let over = |d: u32| {
        let mix = |shift: u32| (((d >> shift) & 0xFF) * 254 + 127) / 255;
        0xFF00_0000 | mix(16) << 16 | mix(8) << 8 | mix(0)
    };
    for (arm, f) in ["instance", "cell", "cpu"].iter().zip(&got) {
        let wrong = (0..h)
            .flat_map(|y| (0..w).map(move |x| (y0 + y) * fw + x0 + x))
            .filter(|&i| f[i] != over(under[i]))
            .count();
        assert_eq!(
            wrong, 0,
            "{arm}: a fill word in an embedder's image is itself"
        );
    }
    arms.renderer.set_theme(base);
    feed(t, b"\x1bc");
}

/// **The hollow cursor on all three paths** (`[cursor] unfocused_hollow`,
/// while another window has the keyboard): each shape, over a bold glyph
/// on a single-width line and on a double-width one, opaque and on the
/// glass, drawn by the instance arm, the cell arm and the CPU arm alike,
/// and not the cursor drawn in its shape. On a single-width line the
/// cell's middle is the frame without a cursor's, and the box's top
/// middle is the text's colour: the cell shader's `cell_ring_vertex` was
/// reached, and swapped nothing.
fn hollow_cursor_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    let base = arms.renderer.theme().clone();
    let fw = usize::try_from(arms.renderer.frame_size().0.value()).unwrap();
    let m = arms.renderer.metrics();
    let (cw, ch) = (
        usize::try_from(m.cell_w.value()).unwrap(),
        usize::try_from(m.cell_h.value()).unwrap(),
    );
    let opaque = Theme {
        bg_alpha: Chan::from_u8(0xFF),
        ..base.clone()
    };
    for (page, theme) in [("opaque", opaque), ("glass", base.clone())] {
        arms.renderer.set_theme(theme);
        for (what, style) in [("block", 2), ("underline", 4), ("bar", 6)] {
            for (line, rendition) in [("single", ""), ("double width", "\x1b#6")] {
                let scene =
                    format!("\x1bc\x1b[2;1H{rendition}\x1b[1mstatus\x1b[0m\x1b[{style} q\x1b[2;3H");
                feed(t, scene.as_bytes());
                feed(t, b"\x1b[?25l");
                let (none, _) = arms.draw(t);
                feed(t, b"\x1b[?25h");
                arms.renderer.set_cursor_look(CursorLook::Shaped);
                let (shaped, _) = arms.draw(t);
                arms.renderer.set_cursor_look(CursorLook::Hollow);
                let (a, b) = arms.draw(t);
                let c = cpu_frame(arms, t);
                let at = format!("{page}, {what}, {line}");
                assert_eq!(differing(&a, &b), 0, "{at}: the cell arm differs");
                let cpu = channel_diff(&c, &a, |_| true);
                assert_eq!(cpu.0, 0, "{at}: the CPU arm differs by up to {}", cpu.1);
                assert!(differing(&a, &shaped) > 0, "{at}: hollow is not the shape");
                if line == "single" {
                    let (x, y) = (2 * cw, ch);
                    let mid = (y + ch / 2) * fw + x + cw / 2;
                    assert_eq!(a[mid], none[mid], "{at}: the cell's middle is its own");
                    let top = y * fw + x + cw / 2;
                    assert_ne!(a[top], none[top], "{at}: the box's top is drawn");
                }
            }
        }
    }
    // A cell a record draws -- a mark joined to its letter -- and an input
    // method's composition with its cursor inside it, each under a hollow
    // block: the records swap nothing, on every arm.
    arms.renderer.set_theme(Theme {
        bg_alpha: Chan::from_u8(0xFF),
        ..base.clone()
    });
    let composing = Composition {
        row: Row::from_index(1),
        col: Col::at_index(1),
        text: "ime"
            .chars()
            .map(|c| baud_domain::Codepoint::new(u32::from(c)).unwrap())
            .collect(),
        measure: baud_render::Measure::default(),
        hints: Vec::new(),
        caret: Caret::At(1),
    };
    let line = usize::try_from(m.thickness.value().max(2)).unwrap();
    let hidden = Composition {
        caret: Caret::Hidden,
        ..composing.clone()
    };
    for (what, scene, comp, without) in [
        (
            "a mark",
            "\x1bc\x1b[2 q\x1b[2;1Hxe\u{301}yz\x1b[2;2H",
            None,
            None,
        ),
        (
            "a composition",
            "\x1bc\x1b[2 q\x1b[2;1Hxyzw\x1b[2;2H",
            Some(composing),
            Some(hidden),
        ),
    ] {
        feed(t, scene.as_bytes());
        // The frame without a cursor: hidden, or the composition's hidden.
        arms.composition = without;
        feed(t, b"\x1b[?25l");
        let (none, _) = arms.draw(t);
        feed(t, b"\x1b[?25h");
        arms.composition = comp;
        arms.renderer.set_cursor_look(CursorLook::Shaped);
        let (shaped, _) = arms.draw(t);
        arms.renderer.set_cursor_look(CursorLook::Hollow);
        let (a, b) = arms.draw(t);
        let c = cpu_frame(arms, t);
        assert_eq!(differing(&a, &b), 0, "{what}: the cell arm differs");
        let cpu = channel_diff(&c, &a, |_| true);
        assert_eq!(cpu.0, 0, "{what}: the CPU arm differs by up to {}", cpu.1);
        assert!(
            differing(&a, &shaped) > 0,
            "{what}: hollow is not the shape"
        );
        // Inside the box, column 2 of row 1, the cell is the frame without
        // a cursor's: nothing swapped.
        let inside = (ch + line..2 * ch - line)
            .flat_map(|y| (2 * cw + line..3 * cw - line).map(move |x| y * fw + x))
            .filter(|&i| a[i] != none[i])
            .count();
        assert_eq!(inside, 0, "{what}: the cell inside the box is its own");
    }
    arms.composition = None;
    arms.renderer.set_cursor_look(CursorLook::Shaped);
    arms.renderer.set_theme(base);
    feed(t, b"\x1bc");
}

/// **A shared Sixel recoloured by a later one, on all three paths**: under
/// `?1070` reset a 16x12 Sixel of register 1's red is drawn red by the
/// instance arm, the cell arm and the CPU arm; a shared body with no data
/// then defines register 1 blue, and the next frame of each draws it blue,
/// the two GPU arms pixel for pixel alike. The recolour moves the store's
/// epoch, which each GPU arm's copy of the image is keyed on: without it
/// they would draw the red they uploaded while the CPU arm, which reads
/// the store, drew blue (`[[source: vt3xx-screen-control]]`, "Output
/// Mapping": "you do not have to redraw an image to change its color").
fn sixel_recolour_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    t.set_sixel_claim(baud_domain::SixelClaim::Yes);
    let fw = usize::try_from(arms.renderer.frame_size().0.value()).unwrap();
    feed(
        t,
        b"\x1bc\x1b[?25l\x1b[?1070l\x1bP9;1q#1;2;100;0;0#1!16~-!16~\x1b\\",
    );
    let frames = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>| {
        let (a, b) = arms.draw(t);
        let c = cpu_frame(arms, t);
        let places = arms.renderer.image_placements();
        assert_eq!(places.len(), 1, "one image on screen");
        let at = (
            usize::try_from(places[0].x).unwrap(),
            usize::try_from(places[0].y).unwrap(),
        );
        ([a, b, c], at)
    };
    for (what, want) in [("drawn", 0xFFFF_0000u32), ("recoloured", 0xFF00_00FF)] {
        if what == "recoloured" {
            feed(t, b"\x1bP9;1q#1;2;0;0;100\x1b\\");
        }
        let (got, (x0, y0)) = frames(arms, t);
        for (arm, f) in ["instance", "cell", "cpu"].iter().zip(&got) {
            for (x, y) in [(0, 0), (15, 11), (7, 6)] {
                assert_eq!(
                    f[(y0 + y) * fw + x0 + x],
                    want,
                    "{what}, {arm} at ({x},{y})"
                );
            }
        }
        assert_eq!(differing(&got[0], &got[1]), 0, "{what}: the GPU arms agree");
    }
    feed(t, b"\x1bc");
    t.set_sixel_claim(baud_domain::SixelClaim::No);
}

/// **A shared Sixel recolours alike on every arm with no foreground said,
/// and while the redefining body is still open**. The terminal here is
/// told no foreground, as a headless embedder may leave it, so a register no
/// Sixel defined paints a clear ink and is drawn as nothing. Picture A
/// draws register 1 undefined, over 16 by 12: until a palette defines it
/// blue, A shows what is under it -- and with `P2` 0 over a 32 by 12
/// raster, the fill -- and once defined A is blue on the instance arm, the
/// cell arm and the CPU arm, the fill past the data unchanged. Then A draws
/// register 1 red, and a palette body arrives in three reads, defining it
/// green in the first and blue in the second and ending in the third: the
/// frame after each read shows green, blue, blue, on every arm, as a
/// VT340's colour map, read at each refresh, shows each definition. The
/// control: the same frames with the definitions not yet sent show none
/// of them.
#[test]
fn a_shared_sixel_recolours_alike_on_every_arm_unsaid_and_mid_body() {
    let font = |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let regular = font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&regular).unwrap()];
    let dark = Theme::dark();
    let opaque = Theme {
        bg_alpha: Chan::from_u8(0xFF),
        ..dark
    };
    let renderer = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        opaque.clone(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    arms!(renderer => arms, t);
    t.set_sixel_claim(baud_domain::SixelClaim::Yes);
    let fw = usize::try_from(arms.renderer.frame_size().0.value()).unwrap();
    let bg = 0xFF00_0000 | (opaque.bg.0 & 0x00FF_FFFF);
    let (red, green, blue) = (0xFFFF_0000u32, 0xFF00_FF00u32, 0xFF00_00FFu32);
    let frames = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>| {
        let (a, b) = arms.draw(t);
        let c = cpu_frame(arms, t);
        let places = arms.renderer.image_placements();
        assert_eq!(places.len(), 1, "one image on screen");
        let at = (
            usize::try_from(places[0].x).unwrap(),
            usize::try_from(places[0].y).unwrap(),
        );
        assert_eq!(differing(&a, &b), 0, "the GPU arms agree");
        ([a, b, c], at)
    };
    let check =
        |got: &[Vec<u32>; 3], (x0, y0): (usize, usize), want: &dyn Fn(u32) -> bool, what: &str| {
            for (arm, f) in ["instance", "cell", "cpu"].iter().zip(got) {
                for (x, y) in [(0, 0), (15, 11), (7, 6)] {
                    let p = f[(y0 + y) * fw + x0 + x];
                    assert!(want(p), "{what}, {arm} at ({x},{y}): {p:#010x}");
                }
            }
        };
    let palette = b"\x1bP9;1q#1;2;0;0;100\x1b\\".as_slice();
    // A line of glyphs far below the picture, which every frame draws.
    const TEXT: &str =
        "\x1bc\x1b[?25l\x1b[?1070l\x1b[8;1Habcdefghijklmnopqrstuvwxyz 0123456789\x1b[H";
    // The pixels of the rectangle `xs` by `ys` of the picture at `at` that
    // are not `want`.
    let unlike = |f: &[u32], (x0, y0): (usize, usize), xs: core::ops::Range<usize>, want: u32| {
        (0..12)
            .flat_map(|y| xs.clone().map(move |x| (y0 + y) * fw + x0 + x))
            .filter(|&i| f[i] != want)
            .count()
    };
    for (p2, raster) in [(1, ""), (0, "\"1;1;32;12")] {
        // Text under the picture, which the fill hides.
        let picture = format!("{TEXT}EEEEEEEE\x1b[H\x1bP9;{p2}q{raster}#1!16~-!16~\x1b\\");
        feed(&mut t, picture.as_bytes());
        let (got, at) = frames(&mut arms, &mut t);
        let what = format!("P2 {p2}, undefined");
        check(&got, at, &|p| p != blue && p != red, &what);
        for (arm, f) in ["instance", "cell", "cpu"].iter().zip(&got) {
            let n = unlike(f, at, 0..32, bg);
            match p2 {
                0 => assert_eq!(
                    n, 0,
                    "{what}, {arm}: the clear ink and the fill hide the text"
                ),
                _ => assert!(n > 0, "{what}, {arm}: the premise, the text under shows"),
            }
        }
        feed(&mut t, palette);
        let (got, at) = frames(&mut arms, &mut t);
        check(&got, at, &|p| p == blue, &format!("P2 {p2}, defined"));
        for (arm, f) in ["instance", "cell", "cpu"].iter().zip(&got) {
            assert_eq!(
                unlike(f, at, 0..16, blue),
                0,
                "P2 {p2}, {arm}: the data, blue"
            );
            if p2 == 0 {
                let n = unlike(f, at, 16..32, bg);
                assert_eq!(n, 0, "{arm}: the fill past the data");
            }
        }
    }
    let picture = format!("{TEXT}\x1bP9;1q#1;2;100;0;0#1!16~-!16~\x1b\\");
    feed(&mut t, picture.as_bytes());
    let (got, at) = frames(&mut arms, &mut t);
    check(&got, at, &|p| p == red, "the premise");
    let reads: [&[u8]; 3] = [b"\x1bP9;1q#1;2;0;100;0#", b"1;2;0;0;100$", b"\x1b\\"];
    for (read, want) in reads.iter().zip([green, blue, blue]) {
        feed(&mut t, read);
        let (got, at) = frames(&mut arms, &mut t);
        let what = format!("after {}", read.escape_ascii());
        check(&got, at, &|p| p == want, &what);
    }
}

/// **The fill goes on as the image scrolls**, on all three paths: a Sixel
/// two rows from the bottom whose data runs past them scrolls the screen,
/// and the rows it scrolls in are filled from the band that overran the
/// rows left to the data's last row rounded up to a cell, across `Ph`,
/// while the rows between `Pv` and that band show what was under them
/// (`[[source: wt-sixel-parser]]`). The scrolled-in rows take the
/// rendition's blue background, which the control, the same Sixel with
/// `P2` 1, shows there. `fw` is the frame's width.
fn sixel_scroll_fill(arms: &mut Arms<'_, '_>, t: &mut Term<'_>, fw: usize) {
    let m = arms.renderer.metrics();
    let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
    let px16 = |v: u32| ImagePx::of(u16::try_from(v).unwrap());
    t.set_cell_size(px16(cw), px16(ch));
    let ch = usize::try_from(ch).unwrap();
    // Two rows left; a band of 6; data past them by more than a cell.
    let left = 2 * ch;
    let bands = 3 * ch / 6 + 1;
    let data_h = 6 * bands;
    let (from, to) = (left / 6 * 6, data_h.div_ceil(ch) * ch);
    let text: &[u8] = b"\x1bc\x1b[?25l\x1b[8;1HEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE\
        \x1b[9;1HEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE\x1b[10;1HEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE\
        \x1b[9;1H\x1b[44m";
    let sixel = |p2: u8| {
        format!(
            "\x1bP9;{p2};0q\"1;1;40;12#1;2;100;0;0#1{}!20~\x1b\\\x1b[m",
            "!20~-".repeat(bands - 1)
        )
    };
    let bg = 0xFF00_0000 | (arms.renderer.theme().bg.0 & 0x00FF_FFFF);
    let mut shot = |p2: u8| {
        feed(t, text);
        feed(t, sixel(p2).as_bytes());
        let (a, b) = arms.draw(t);
        let c = cpu_frame(arms, t);
        let places = arms.renderer.image_placements();
        assert_eq!(places.len(), 1, "one image on screen");
        let p = places[0];
        let at = (usize::try_from(p.x).unwrap(), usize::try_from(p.y).unwrap());
        ([a, b, c], at, usize::from(p.h))
    };
    let (control, at, h) = shot(1);
    assert_eq!(h, data_h, "P2 1: the data's reach");
    let (got, at0, h) = shot(0);
    assert_eq!(at0, at, "the fill does not move the image");
    let (x0, y0) = at;
    let region = |rows: core::ops::Range<usize>| {
        rows.flat_map(move |y| (20..40).map(move |x| (y0 + y) * fw + x0 + x))
            .collect::<Vec<_>>()
    };
    let blue = |f: &[u32]| region(left..to).into_iter().filter(|&i| f[i] != bg).count();
    assert!(
        blue(&control[0]) > 0,
        "the premise: P2 1 shows the blue rows"
    );
    for (arm, (f, c)) in ["instance", "cell", "cpu"]
        .iter()
        .zip(got.iter().zip(&control))
    {
        let unfilled = region(from..to).into_iter().filter(|&i| f[i] != bg).count();
        assert_eq!(unfilled, 0, "{arm}: the scrolled-in rows are filled");
        let between = region(12..from)
            .into_iter()
            .filter(|&i| f[i] != c[i])
            .count();
        assert_eq!(between, 0, "{arm}: between Pv and the band, what was under");
    }
    assert_eq!(h, to, "P2 0: to the cell the data ends in");
    t.set_cell_size(ImagePx::ZERO, ImagePx::ZERO);
    feed(t, b"\x1bc");
}

/// One run of [`glass_floor_gate`]: the theme at rest, a text and page
/// pair the program sets that the floor lifts at that theme's alpha, and a
/// line colour the floor lifts on the default page.
struct GlassCase {
    name: &'static str,
    resting: Theme,
    text: (u8, u8, u8),
    page: (u8, u8, u8),
    line: (u8, u8, u8),
}

/// [`glass_floor_gate`]'s three runs: the old default dim and the two
/// shipped since 2026-10-17, `dim_dark` 0.70 (alpha 179) under the dark
/// theme and `dim_light` 0.60 (alpha 153) under the light one. Each pair
/// reads 3:1 or above as asked and below 3 at its alpha against
/// everything its page can show over the glass, from the page over black
/// to the page over white: gruvbox-light's 10.22 to 1.00 at 56,
/// `#303030` on `#888888` 3.72 to 2.07 at 179, `#a0a0a0` on `#303030`
/// 5.05 to 1.45 at 153. Gruvbox-light's pair is not lifted at either
/// shipped alpha (4.91 and 3.66), which is why each has its own. The
/// lines read 1.05:1 and 1.06:1 on their default pages.
fn glass_cases(resting: &Theme) -> [GlassCase; 3] {
    assert_eq!(
        resting.bg_alpha.get(),
        56,
        "the premise: alpha 56, the pre-2026-10-17 default"
    );
    let shipped = |alpha: u8, theme: Theme| Theme {
        bg_alpha: Chan::from_u8(alpha),
        app_bg: resting.app_bg,
        ..theme
    };
    [
        GlassCase {
            name: "alpha 56, dark",
            resting: resting.clone(),
            text: (60, 56, 54),
            page: (251, 241, 199),
            line: (21, 22, 27),
        },
        GlassCase {
            name: "alpha 179, dark",
            resting: shipped(179, Theme::dark()),
            text: (48, 48, 48),
            page: (136, 136, 136),
            line: (21, 22, 27),
        },
        GlassCase {
            name: "alpha 153, light",
            resting: shipped(153, Theme::light()),
            text: (160, 160, 160),
            page: (48, 48, 48),
            line: (232, 233, 234),
        },
    ]
}

/// **The floor on the page the glass draws**, on all three arms, at each
/// of [`glass_cases`]: a text and page pair the program sets, drawn as a
/// 2x20 picture of forty pages near the pair's, one cell of it a marked
/// glyph the cell arm draws from a record, as a one-page panel, under a
/// selection and under a block cursor. The panel's glyphs are lifted, so
/// they differ from the same frame with the floor off; the picture's, the
/// selection's and the cursor's are drawn solid and take the text asked,
/// so they do not; nor do box-drawing lines, on the panel and in the
/// case's line colour on the default page, while two block elements in
/// that colour are floored. The two GPU arms read one resolved table but
/// choose between its two texts in two spellings, `Resolved::drawn` and
/// the shader's `colors()`, and the CPU arm composites the instance arm's
/// colours: this compares all three.
fn glass_floor_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>, case: &GlassCase) {
    let GlassCase {
        name,
        ref resting,
        text: (tr, tg, tb),
        page: (pr, pg, pb),
        line: (lr, lg, lb),
    } = *case;
    assert_eq!(arms.margin, (0, 0), "the premise: the frame at the origin");
    let pair = format!("38;2;{tr};{tg};{tb};48;2;{pr};{pg};{pb}");
    feed(t, b"\x1b[?25l\x1b[2J\x1b[H\x1b[2 q");
    for y in 0..2u32 {
        feed(t, format!("\x1b[{};3H", y + 2).as_bytes());
        for x in 0..20u32 {
            // One marked glyph, which the cell arm draws from a record.
            let glyph = if (x, y) == (5, 0) { "e\u{301}" } else { "X" };
            feed(
                t,
                format!(
                    "\x1b[38;2;{tr};{tg};{tb};48;2;{};{};{pb}m{glyph}",
                    u32::from(pr) - x,
                    u32::from(pg) - y * 8
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
    // The line colour on the default page (herdr's divider under the dark
    // theme): four box-drawing lines, one of them DEC Special Graphics'
    // `q`, then two block elements.
    feed(
        t,
        format!(
            "\x1b[7;3H\x1b[38;2;{lr};{lg};{lb}m\u{2500} \u{2502} \u{256d} \u{257f} \x1b(0q\x1b(B \u{2580} \u{2588}\x1b[0m"
        )
        .as_bytes(),
    );
    feed(t, b"\x1b[5;14H\x1b[?25h");
    arms.highlight = Some(Highlight::new(
        (Row::from_index(4), Col::at_index(2)),
        (Row::from_index(4), Col::at_index(6)),
        Region::Stream,
    ));
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
            "{name}, {floor:?}: the cell arm's overlay found the picture"
        );
        let bx = diff_box(&a, &b, fw, (cw, ch));
        assert_eq!(
            differing(&a, &b),
            0,
            "{name}, {floor:?}: the GPU arms disagree over the glass floor, cells {bx:?}"
        );
        let cpu = cpu_frame(arms, t);
        let all = channel_diff(&cpu, &a, |_| true);
        assert_eq!(
            all.0, 0,
            "{name}, {floor:?}: CPU and GPU differ on {} pixels, by up to {}",
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
    assert!(
        moved(4..5, 8..11) > 0,
        "{name}: the panel's glyphs were not lifted"
    );
    assert_eq!(
        moved(4..5, 18..19),
        0,
        "{name}: a line on the panel was lifted"
    );
    assert_eq!(
        moved(6..7, 2..11),
        0,
        "{name}: a box-drawing line was floored"
    );
    for col in [12, 14] {
        assert!(
            moved(6..7, col..col + 1) > 0,
            "{name}: the block element at {col} was not floored"
        );
    }
    assert_eq!(moved(1..3, 2..22), 0, "{name}: a picture's text was lifted");
    assert_eq!(
        moved(4..5, 2..7),
        0,
        "{name}: a selected cell's text was lifted"
    );
    assert_eq!(
        moved(4..5, 13..14),
        0,
        "{name}: the block cursor's cell was lifted"
    );

    // **A bar cursor on a line keeps the floored colour it always had**: the
    // line rule is the text's, not the cursor's. All three arms draw it, and
    // the bar's pixels move with the floor. A cursor of no colour of its
    // own, which takes the text's; baud's amber is not floored.
    feed(t, b"\x1b[7;3H\x1b[6 q\x1b[?25h");
    let mut bars = Vec::new();
    for floor in [baud_render::Floor::DEFAULT, baud_render::Floor::Off] {
        arms.renderer.set_theme(Theme {
            app_bg: baud_render::AppBg::Dimmed,
            picture_bg: baud_render::PictureBg::Opaque,
            floor,
            cursor: None,
            cursor_text: None,
            cursor_made: baud_render::CursorMade::Named,
            ..resting.clone()
        });
        let (a, b) = arms.draw(t);
        assert_eq!(
            differing(&a, &b),
            0,
            "{name}, {floor:?}: the GPU arms disagree over a bar cursor on a line"
        );
        let cpu = cpu_frame(arms, t);
        let all = channel_diff(&cpu, &a, |_| true);
        assert_eq!(
            all.0, 0,
            "{name}, {floor:?}: CPU and GPU differ under a bar cursor on a line, by up to {}",
            all.1
        );
        bars.push(b[6 * ch * fw + 2 * cw]);
    }
    assert_ne!(
        bars[0], bars[1],
        "{name}: the bar cursor on a line was drawn as asked, not floored"
    );
    feed(t, b"\x1b[?25l\x1b[0 q");

    // **A selected line's background is the floored text, as a selected
    // letter's is**: a swap hands the background `fg` and the line rule
    // leaves it alone. Row 8 holds `x` and `U+2502` in herdr's divider
    // colour, which says where each glyph inks; row 9 holds them again,
    // selected. All three arms draw it; on the pixels neither glyph inks,
    // the selected line's equal the selected letter's, and move with the
    // floor.
    feed(
        t,
        format!("\x1b[9;41H\x1b[38;2;{lr};{lg};{lb}mx\u{2502}\x1b[10;41Hx\u{2502}\x1b[0m")
            .as_bytes(),
    );
    arms.highlight = Some(Highlight::new(
        (Row::from_index(9), Col::at_index(40)),
        (Row::from_index(9), Col::at_index(41)),
        Region::Stream,
    ));
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
            "{name}, {floor:?}: the GPU arms disagree over a selected line"
        );
        let cpu = cpu_frame(arms, t);
        let all = channel_diff(&cpu, &a, |_| true);
        assert_eq!(
            all.0, 0,
            "{name}, {floor:?}: CPU and GPU differ over a selected line, by up to {}",
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
        "{name}: the control: most of the cell is bare, {} of {}",
        bare.len(),
        cw * ch
    );
    let pick = |floor: usize, k: usize| bare.iter().map(|&i| seen[floor][k][i]).collect::<Vec<_>>();
    assert_eq!(
        pick(0, 4),
        pick(0, 3),
        "{name}: a selected line's background is a selected letter's, floored"
    );
    assert_ne!(
        pick(0, 4),
        pick(1, 4),
        "{name}: the control: the floor moved the selected line's background"
    );
}

/// **A repeat pass makes no flash transition, on the GPU's own pixels** --
/// the flash bound's pixel check (note AL, option O2), on both GPU arms:
/// every effect at three ages, a black cursor (`OSC 12`) over the light
/// theme's opaque page and whatever text the scene left under it. Against
/// the frame without a trail, outside the cursor's cell, no pixel of a
/// repeat pass is a WCAG 2.2 general transition (a change in relative
/// luminance of 0.10 or more, the darker below 0.80). A repeat pass draws
/// its bands at the cap of the colours under them and drops its particles,
/// whose alphas would compound over the bands. The control: the same looks
/// at full strength make transitions.
fn repeat_pass_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    let (resting, margin) = (arms.renderer.theme().clone(), arms.margin);
    arms.margin = (0, 0);
    arms.renderer.set_theme(Theme::light());
    feed(t, b"\x1b]12;rgb:00/00/00\x07");
    let m = arms.renderer.metrics();
    let (cw, ch) = (m.cell_w.value() as f32, m.cell_h.value() as f32);
    let cell_at = |col: f32, row: f32| Rect {
        left: col * cw,
        top: row * ch,
        right: (col + 1.0) * cw,
        bottom: (row + 1.0) * ch,
    };
    let (from, to) = (cell_at(98.0, 2.0), cell_at(116.0, 3.0));
    let fw = usize::try_from(arms.renderer.frame_size().0.value()).unwrap();
    arms.trail = None;
    let (plain, _) = arms.draw(t);
    let lum = |p: u32| {
        let [_, r, g, b] = p.to_be_bytes();
        let lin = |c: u8| {
            let c = f64::from(c) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
    };
    let transitions = |frame: &[u32]| {
        frame
            .iter()
            .zip(&plain)
            .enumerate()
            .filter(|&(i, (&p, &q))| {
                let (x, y) = ((i % fw) as f32 + 0.5, (i / fw) as f32 + 0.5);
                let hole = x > to.left && x < to.right && y > to.top && y < to.bottom;
                let (a, b) = (lum(p), lum(q));
                !hole && (a - b).abs() >= 0.10 && a.min(b) < 0.80
            })
            .count()
    };
    let mut full = 0;
    for effect in Effect::ALL {
        let ignite = Ignition::ignite(
            Pt {
                x: (from.left + from.right) * 0.5,
                y: (from.top + from.bottom) * 0.5,
            },
            to,
            Seed(7),
            ch,
        );
        let life = (effect.end(&ignite) / Effect::Fire.life()).min(1.0);
        for age in [0.04f32, 0.12, 0.25].map(|a| a * life) {
            for strength in [
                baud_render::Strength::Full,
                baud_render::Strength::UnderThreshold,
            ] {
                arms.trail = Some(TrailLook::Vfx(Vfx {
                    effect,
                    emitter: Smear {
                        corners: Segment::new(from.corners(), to).at(age),
                        hole: to,
                        shape: Shape::Quad,
                        strength,
                        opacity: Smear::OPAQUE,
                    },
                    ignition: ignite.at(age),
                    axis: ignite.dir,
                }));
                let (a, b) = arms.draw(t);
                assert_eq!(
                    differing(&a, &b),
                    0,
                    "{effect:?} at {age}, {strength:?}: the GPU arms disagree"
                );
                let n = transitions(&b);
                match strength {
                    baud_render::Strength::Full => full += n,
                    baud_render::Strength::UnderThreshold => assert_eq!(
                        n, 0,
                        "{effect:?} at {age}: a repeat pass flashes {n} pixels"
                    ),
                }
            }
        }
    }
    assert!(full > 0, "the control: full passes flash");
    arms.trail = None;
    feed(t, b"\x1b]112\x07");
    arms.renderer.set_theme(resting);
    arms.margin = margin;
}

/// **The exit-status strip on every path**: a session whose commands
/// succeed, fail and are cancelled, with the marks on, drawn by the
/// instance arm, the cell arm and the CPU arm into the whole frame, the
/// strip at its left. The three agree on every pixel, the strip holds a
/// mark of each kind (a positive control), and the grid beside it is the
/// frame drawn with the marks off, moved right by the strip; off again,
/// the frame is that one.
fn status_marks_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    let margin = arms.margin;
    arms.margin = (0, 0);
    feed(t, b"\x1b[2J\x1b[H");
    feed(
        t,
        b"\x1b]133;A\x07$ \x1b]133;B\x07ok\x1b]133;C\x07\r\nout\r\n\x1b]133;D;0\x07\
          \x1b]133;A\x07$ \x1b]133;B\x07bad\x1b]133;C\x07\r\nerr\r\n\x1b]133;D;2\x07\
          \x1b]133;A\x07$ \x1b]133;B\x07half\r\n\x1b]133;D\x07\
          \x1b]133;A\x07$ ",
    );
    let (off_a, _) = arms.draw(t);
    let (fw, fh) = arms.renderer.whole_size();
    let (fw, fh) = (
        usize::try_from(fw.value()).unwrap(),
        usize::try_from(fh.value()).unwrap(),
    );
    arms.renderer.set_status_marks(StatusMarks::On);
    let (a, b) = arms.draw(t);
    let c = cpu_frame(arms, t);
    assert_eq!(
        arms.renderer.strip_records().len(),
        3,
        "three ended commands"
    );
    let (w, h) = arms.renderer.whole_size();
    let (w, h) = (
        usize::try_from(w.value()).unwrap(),
        usize::try_from(h.value()).unwrap(),
    );
    let s = usize::try_from(arms.renderer.strip().value()).unwrap();
    assert_eq!((w, h), (fw + s, fh));
    assert_eq!(differing(&a, &b), 0, "the GPU arms agree with the strip");
    assert_eq!(differing(&a, &c), 0, "the CPU arm agrees with the strip");
    let ch = usize::try_from(arms.renderer.metrics().cell_h.value()).unwrap();
    let clear = a[0];
    let strip_row = |p: &[u32], row: usize| -> Vec<u32> {
        (row * ch..(row + 1) * ch)
            .flat_map(|y| p[y * w..y * w + s].to_vec())
            .collect()
    };
    for (row, marked) in [(0, true), (1, false), (2, true), (4, true), (5, false)] {
        let any = strip_row(&a, row).iter().any(|&p| p != clear);
        assert_eq!(any, marked, "row {row} of the strip");
    }
    assert_ne!(
        strip_row(&a, 0),
        strip_row(&a, 2),
        "success and failure differ"
    );
    for y in 0..h {
        assert_eq!(
            &a[y * w + s..y * w + w],
            &off_a[y * fw..y * fw + fw],
            "grid row {y}"
        );
    }
    arms.renderer.set_status_marks(StatusMarks::Off);
    let (again, _) = arms.draw(t);
    assert_eq!(again, off_a, "off draws the frame without the strip");
    feed(t, b"\x1b[2J\x1b[H");
    arms.margin = margin;
}

/// **The word wrap's continuation marks on every path**: an English
/// paragraph and a Hebrew one the terminal cut inside a word, finished
/// above a marked prompt, re-wrapped at their words, with a band of
/// padding lent each side, drawn by the instance arm, the cell arm and the
/// CPU arm into the whole frame. The three agree on every pixel, the
/// English continuation is marked in the left margin and the Hebrew in the
/// right (a positive control each), the grid between is the frame drawn
/// with no margins, moved right by the band, and with none lent again the
/// frame is that one.
fn wrap_marks_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    let (margin, reads) = (arms.margin, arms.reads);
    arms.margin = (0, 0);
    // The cell arm reads the display's lines, a copy, as the app's does
    // for a window it re-wraps.
    arms.reads = Reads::Copy;
    let three = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, what: &str| {
        let (a, _) = arms.draw(t);
        let b = arms.draw_copied(t);
        let c = cpu_frame(arms, t);
        assert_eq!(differing(&a, &b), 0, "{what}: the cell arm differs");
        assert_eq!(differing(&a, &c), 0, "{what}: the CPU arm differs");
        a
    };
    feed(t, b"\x1b[2J\x1b[H");
    let he = "\u{5E9}\u{5DC}\u{5D5}\u{5DD} \u{5E2}\u{5D5}\u{5DC}\u{5DE}\u{5D9} ".repeat(13);
    let text = format!("{}\r\n{he}\r\n\x1b]133;A\x07$ ", "alphabet ".repeat(20));
    feed(t, text.as_bytes());
    let off = three(arms, t, "no margins");
    let (fw, fh) = arms.renderer.whole_size();
    let (fw, fh) = (
        usize::try_from(fw.value()).unwrap(),
        usize::try_from(fh.value()).unwrap(),
    );
    let band = 16usize;
    arms.renderer.set_margins(Px::of(16));
    let a = three(arms, t, "the marks");
    assert!(
        arms.renderer.margin_records().len() >= 2,
        "both paragraphs continue"
    );
    let (w, h) = arms.renderer.whole_size();
    let (w, h) = (
        usize::try_from(w.value()).unwrap(),
        usize::try_from(h.value()).unwrap(),
    );
    assert_eq!((w, h), (fw + 2 * band, fh));
    let clear = a[0];
    let inked =
        |x0: usize| (0..h).any(|y| a[y * w + x0..y * w + x0 + band].iter().any(|&p| p != clear));
    assert!(inked(0), "the English line marked on the left");
    assert!(inked(w - band), "the Hebrew line marked on the right");
    for y in 0..h {
        assert_eq!(
            &a[y * w + band..y * w + band + fw],
            &off[y * fw..y * fw + fw],
            "grid row {y}"
        );
    }
    arms.renderer.set_margins(Px::of(0));
    let again = three(arms, t, "none lent");
    assert_eq!(again, off, "none lent draws the frame without margins");
    feed(t, b"\x1b[2J\x1b[H");
    arms.margin = margin;
    arms.reads = reads;
}

/// **The activity lights' own line on every path**: with the line below
/// the grid and then above it, the instance arm, the cell arm and the CPU
/// arm agree on every pixel of the whole frame, the line holds the plate
/// and the lights (a positive control) -- so every lamp pixel the CPU arm
/// draws as its palette entry the GPU arms draw so too -- and the grid's
/// frame beside it is the frame drawn
/// with no line, at the whole's top or the line's height down; off again,
/// the frame is that one. The pictures reach the GPU on the colour sheet
/// and no shader is added: the line's records are `Kind::Color`.
fn lights_line_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    use baud_render::{ByLed, Glow, Led, Light, LineEdge, Shown};
    let margin = arms.margin;
    arms.margin = (0, 0);
    feed(t, b"\x1b[2J\x1b[H$ ls\r\nfiles\r\n$ ");
    let (off_a, _) = arms.draw(t);
    let (fw, _) = arms.renderer.whole_size();
    let fw = usize::try_from(fw.value()).unwrap();
    let shown = ByLed {
        rd: Light::Lit,
        oh: Light::Lit,
        cd: Light::Unread,
        ..Shown::DARK
    };
    arms.renderer.set_lights(shown, Glow::Full);
    for edge in [LineEdge::Bottom, LineEdge::Top] {
        arms.renderer.set_lights_line(Some(edge), 1.0);
        let (a, b) = arms.draw(t);
        let c = cpu_frame(arms, t);
        assert!(
            arms.renderer
                .lights_records()
                .iter()
                .all(|r| r.kind == Kind::Color),
            "{edge:?}: pictures on the colour sheet"
        );
        assert_eq!(
            arms.renderer.lights_records().len(),
            1 + Led::ALL.len(),
            "{edge:?}: the plate, then the lights"
        );
        let line = usize::try_from(arms.renderer.lights_height().value()).unwrap();
        let (w, h) = arms.renderer.whole_size();
        let (w, h) = (
            usize::try_from(w.value()).unwrap(),
            usize::try_from(h.value()).unwrap(),
        );
        assert_eq!((w, h), (fw, off_a.len() / fw + line), "{edge:?}");
        assert_eq!(differing(&a, &b), 0, "{edge:?}: the GPU arms agree");
        assert_eq!(differing(&a, &c), 0, "{edge:?}: the CPU arm agrees");
        let (line_at, grid_at) = match edge {
            LineEdge::Bottom => (h - line, 0),
            LineEdge::Top => (0, line),
        };
        let clear = a[(line_at + line - 1) * w];
        assert!(
            a[line_at * w..(line_at + line) * w]
                .iter()
                .any(|&p| p != clear),
            "{edge:?}: the lights on the line"
        );
        assert_eq!(
            &a[grid_at * w..(grid_at * w + off_a.len())],
            &off_a[..],
            "{edge:?}: the grid's frame is the one with no line"
        );
    }
    arms.renderer.set_lights_line(None, 1.0);
    let (again, _) = arms.draw(t);
    assert_eq!(again, off_a, "off draws the frame without the line");
    feed(t, b"\x1b[2J\x1b[H");
    arms.margin = margin;
}

/// **Smooth scrolling slides the grid and holds the lights' line still**,
/// below the grid and above it, on the cell path that slides: a full-row
/// slide at the live screen is the frame one line up, the line included,
/// so the leaving row is not drawn over a line above the grid and no row
/// pans over one below it; and half-way, the line's rows are the settled
/// frame's while the grid's are not (the control).
fn lights_line_slide_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    use baud_render::{ByLed, Glow, Light, LineEdge, Shown};
    let margin = arms.margin;
    arms.margin = (0, 0);
    feed(t, b"\x1b[?25l\x1b[2J\x1b[H");
    for i in 0..14u32 {
        feed(
            t,
            format!("0000 1111 2222 row {i:02} 3333 4444 5555\r\n").as_bytes(),
        );
    }
    let cell_h = arms.renderer.metrics().cell_h;
    arms.renderer
        .sync_styles(t, arms.styles_mem, &mut arms.stamp);
    arms.map.ensure(
        t,
        t.view(Rows::of_count(1)),
        arms.styles_mem,
        &mut arms.renderer,
        &mut arms.candidates,
    );
    arms.renderer.set_lights(
        ByLed {
            rd: Light::Lit,
            sd: Light::Lit,
            ..Shown::DARK
        },
        Glow::Full,
    );
    for edge in [LineEdge::Bottom, LineEdge::Top] {
        arms.renderer.set_lights_line(Some(edge), 1.0);
        let (w, h) = arms.renderer.whole_size();
        let (w, h) = (
            usize::try_from(w.value()).unwrap(),
            usize::try_from(h.value()).unwrap(),
        );
        let line = usize::try_from(arms.renderer.lights_height().value()).unwrap();
        let line_at = usize::try_from(arms.renderer.lights_top().value()).unwrap();
        let rows_of = |p: &[u32], at: usize, n: usize| p[at * w..(at + n) * w].to_vec();
        arms.back = Rows::ZERO;
        arms.slide = Some(cell_h);
        let (_, full_slid) = arms.draw(t);
        arms.slide = None;
        arms.back = Rows::of_count(1);
        let (_, one_up) = arms.draw(t);
        arms.back = Rows::ZERO;
        assert_eq!(
            differing(&full_slid, &one_up),
            0,
            "{edge:?}: a full-row slide is the screen one line up, its line still"
        );
        arms.slide = Some(Px::of(cell_h.value() / 2));
        let (_, mid) = arms.draw(t);
        arms.slide = None;
        let (_, settled) = arms.draw(t);
        assert_eq!(
            rows_of(&mid, line_at, line),
            rows_of(&settled, line_at, line),
            "{edge:?}: the line holds still mid-slide"
        );
        assert!(differing(&mid, &settled) > 0, "{edge:?}: the grid slid");
        assert_eq!(mid.len(), w * h);
    }
    arms.renderer.set_lights_line(None, 1.0);
    feed(t, b"\x1b[2J\x1b[H\x1b[?25h");
    arms.margin = margin;
}

/// **`SUB`'s mark draws as the character it names, on all three arms**:
/// with the mark on, `a SUB b` in bold red is the frame of `a`, the
/// character and `b` typed in that rendition, and with it off the frame of
/// `ab` -- the control that the mark reached the pixels. DEC's U+2426, which
/// this stack has no glyph for and draws as the placeholder TUS §5.3 asks
/// for, and a `?` the stack draws as a glyph.
fn substitute_mark_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    use baud_domain::{Codepoint, ErrorCharacter, SubstituteMark};
    let q = ErrorCharacter::of(Codepoint::from_char('?')).expect("one cell");
    for (c, typed) in [(ErrorCharacter::DEC, "\u{2426}"), (q, "?")] {
        let frame = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, text: &str| {
            feed(t, b"\x1b[?25l\x1b[2J\x1b[H\x1b[1;31m");
            feed(t, text.as_bytes());
            feed(t, b"\x1b[m");
            let (a, b) = arms.draw(t);
            let c = cpu_frame(arms, t);
            assert_eq!(differing(&a, &b), 0, "{typed}: the GPU arms agree");
            assert_eq!(differing(&a, &c), 0, "{typed}: the CPU arm agrees");
            a
        };
        t.set_substitute_mark(SubstituteMark::Off);
        let off = frame(arms, t, "a\x1ab");
        let bare = frame(arms, t, "ab");
        assert_eq!(differing(&off, &bare), 0, "{typed}: off draws nothing");
        t.set_substitute_mark(SubstituteMark::Drawn(c));
        let on = frame(arms, t, "a\x1ab");
        let want = frame(arms, t, &format!("a{typed}b"));
        assert_eq!(
            differing(&on, &want),
            0,
            "{typed}: the mark is the character"
        );
        assert!(
            differing(&on, &bare) > 0,
            "{typed}: the mark reached the pixels"
        );
    }
    t.set_substitute_mark(SubstituteMark::Off);
    feed(t, b"\x1b[2J\x1b[H");
}

/// The window copied flat into `copy`, as the app copies it: the
/// screen's rows in the order each is drawn in, then the leaving row of a
/// slide or the status line's, then the config's notice row, blank. How
/// many rows it holds.
fn copy_window(
    t: &Term<'_>,
    back: Rows,
    slide: Option<Px>,
    renderer: &mut Renderer<'_, '_>,
    copy_mem: &mut [PackedCell],
) -> usize {
    // The window in its drawn order, as the app reads it: the display's
    // lines where it re-broke a paragraph (`Orders::window`).
    let (mut cells, mut used) = (Vec::new(), Vec::new());
    let stride = t.stride().index();
    let view = renderer.bidi().window(t, back, &mut cells, &mut used);
    let (cols, rows) = (view.cols().index(), view.rows().index());
    for y in 0..rows {
        let r = Row::from_index(y);
        let at = y * stride;
        copy_mem[at..at + cols].copy_from_slice(&view.row(r)[..cols]);
    }
    // Smooth scroll: append the leaving row -- the line just above the
    // window, `view(back + 1).row(0)` -- at index `rows`, and read one
    // more row, so the shader's `slide_rows = 1` maps draw row 0 to it
    // with the identity rotation. Copied as the app copies it, in the order
    // it was drawn in as the first row of the window one line up, a marked
    // cell given its base (`Renderer::copy_leaving`).
    let ring = if slide.is_some() {
        let at = rows * stride;
        renderer.copy_leaving(t, back, &mut copy_mem[at..at + cols]);
        rows + 1
    } else if let Some(line) = t.status_line() {
        // The status line's row after the screen's, as the app copies
        // it, in its drawn order; no slide runs while it is shown.
        let at = rows * stride;
        let mut drawn = Vec::new();
        let line = renderer
            .bidi()
            .status_line(Row::from_index(rows), line, &mut drawn);
        copy_mem[at..at + cols].copy_from_slice(&line.cells[..cols]);
        rows + 1
    } else {
        rows
    };
    // The config's notice row after both, blank, as the app copies it.
    match renderer.notice_bar() {
        Some(_) => {
            let at = ring * stride;
            copy_mem[at..at + cols].fill(PackedCell::EMPTY);
            ring + 1
        }
        None => ring,
    }
}

/// The CPU arm: [`Renderer::render_at`] into a [`Target`], each pixel then
/// put through the macOS presenter's own `premultiply`, as
/// `Presenter::present` copies a frame into its surface -- the form the
/// two GPU arms' readback is in.
fn cpu_frame(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) -> Vec<u32> {
    let (fw, fh) = arms.renderer.whole_size();
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
/// the glass page at three dims**: the old default (`[glass] dim_dark =
/// 0.22`, alpha 56) and the two shipped since 2026-10-17, `dim_dark` 0.70
/// (alpha 179) under the dark theme and `dim_light` 0.60 (alpha 153) under
/// the light one. Text in
/// the default and in set colours, a block cursor, and a 40x30 image of
/// partly transparent pixels over the text; then over the glass the same
/// image over text on a set background, dimmed as the app's default
/// `AppBg::Dimmed` dims it; and over each, translucent cursors: a bar and
/// an underline half faded, a block near the end of a fade, the plasma's
/// glow, and the fire's and the ufo's solids at three points each.
/// Source-over is W3C Compositing and Blending Level 1's 5.1,
/// `co = Cs as + Cb ab (1 - as)` premultiplied,
/// `ao = as + ab (1 - as)` (`[[source: w3c-compositing-1]]`), which the GPU
/// arms' `One, OneMinusSourceAlpha` blend is over premultiplied pixels; the
/// CPU arm keeps straight pixels and the presenter premultiplies them, so
/// its straight result must be the one that premultiplies to the GPU's.
///
/// Over the opaque page the control: every pixel equal. Over the glass page
/// at alpha 56 a blend that mixes the page's colour as if it were opaque differs from
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
        "the premise: alpha 56, the pre-2026-10-17 default"
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
    let light = Theme {
        app_bg: glass.app_bg,
        ..Theme::light()
    };
    let dimmed = baud_render::AppBg::Dimmed;
    for (page, base, alpha, app_bg, text) in [
        ("opaque", &glass, 0xFF, glass.app_bg, plain),
        ("glass", &glass, 56, glass.app_bg, plain),
        ("glass, set backgrounds", &glass, 56, dimmed, set),
        ("glass 179", &glass, 179, glass.app_bg, plain),
        ("glass 179, set backgrounds", &glass, 179, dimmed, set),
        ("light glass 153", &light, 153, glass.app_bg, plain),
        ("light glass 153, set backgrounds", &light, 153, dimmed, set),
    ] {
        arms.renderer.set_theme(Theme {
            bg_alpha: Chan::from_u8(alpha),
            app_bg,
            ..base.clone()
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
        // bar and underline half way through a fade, a block near the end
        // of one, the plasma's glow, and the fire and the ufo part way
        // through (a steady style neither fades nor glows).
        let phase = |opacity: u8, glow: u8| CursorPhase {
            opacity: Chan::from_u8(opacity),
            glow: Chan::from_u8(glow),
            ..CursorPhase::HIDDEN
        };
        let fire = |at: u8| CursorPhase::fire(Chan::from_u8(at));
        let ufo = |at: u8| CursorPhase::ufo(Chan::from_u8(at));
        for (shape, p, what) in [
            (b"\x1b[5 q", phase(128, 0), "a bar half faded"),
            (b"\x1b[3 q", phase(128, 0), "an underline half faded"),
            (b"\x1b[1 q", phase(40, 0), "a block near the end of a fade"),
            (b"\x1b[1 q", phase(0, 200), "a block's glow"),
            (b"\x1b[1 q", fire(70), "a block burning down"),
            (b"\x1b[1 q", fire(135), "a block in embers"),
            (b"\x1b[1 q", fire(205), "a block relighting"),
            (b"\x1b[1 q", ufo(70), "a block lifting"),
            (b"\x1b[1 q", ufo(128), "a block held up"),
            (b"\x1b[1 q", ufo(190), "a block setting down"),
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
            fg: base.fg.with_alpha(Chan::from_u8(180)),
            ..base.clone()
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
/// same cell, each whole and half faded, as a hidden cursor's trail fades,
/// and the fading ghost -- each drawn by both GPU arms and the CPU arm,
/// which paints the quad and the sweep by `trail::coverage` per pixel
/// (`Target::quad`) and the ghost as a translucent solid. A half faded
/// smear differs from the page and from itself whole, and each effect half
/// faded is drawn alike by both GPU arms, between the page and itself
/// whole at every pixel and strictly between at some.
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
                strength: baud_render::Strength::Full,
                opacity: Smear::OPAQUE,
            }),
            "the trail's quad",
        ),
        (
            TrailLook::Smear(Smear {
                corners: from.corners(),
                hole,
                shape: Shape::Sweep,
                strength: baud_render::Strength::Full,
                opacity: Smear::OPAQUE,
            }),
            "the trail's sweep",
        ),
        (
            TrailLook::Smear(Smear {
                corners: seg.at(0.05),
                hole,
                shape: Shape::Quad,
                strength: baud_render::Strength::Full,
                opacity: Chan::from_u8(0x80),
            }),
            "the trail's quad half faded",
        ),
        (
            TrailLook::Smear(Smear {
                corners: from.corners(),
                hole,
                shape: Shape::Sweep,
                strength: baud_render::Strength::Full,
                opacity: Chan::from_u8(0x80),
            }),
            "the trail's sweep half faded",
        ),
        (
            TrailLook::Ghost(Trail {
                from: (Row::from_index(1), Col::at_index(14)),
                to: (cr, cc),
                progress: 0.5,
                origin_dy: 0.0,
                lift: Px::of(0),
                strength: baud_render::Strength::Full,
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
        match look {
            TrailLook::Smear(s) if s.opacity != Smear::OPAQUE => {
                arms.trail = Some(TrailLook::Smear(Smear {
                    opacity: Smear::OPAQUE,
                    ..s
                }));
                let (whole, _) = arms.draw(t);
                assert!(
                    differing(&a, &whole) > 0,
                    "{page}, {what}: drawn as it is whole"
                );
            }
            _ => {}
        }
    }
    let unit = f32::from(u16::try_from(arms.renderer.metrics().cell_h.value()).unwrap());
    let tail = Pt {
        x: (from.left + from.right) * 0.5,
        y: (from.top + from.bottom) * 0.5,
    };
    for effect in Effect::ALL {
        let ig = Ignition::ignite(tail, hole, Seed(7), unit);
        let age = 0.2 * effect.end(&ig);
        let look = |opacity| {
            TrailLook::Vfx(Vfx {
                effect,
                emitter: Smear {
                    corners: seg.at(age),
                    hole,
                    shape: Shape::Quad,
                    strength: baud_render::Strength::Full,
                    opacity,
                },
                ignition: ig.at(age),
                axis: ig.dir,
            })
        };
        arms.trail = Some(look(Smear::OPAQUE));
        let (whole, _) = arms.draw(t);
        arms.trail = Some(look(Chan::from_u8(0x80)));
        let (a, b) = arms.draw(t);
        assert_eq!(
            differing(&a, &b),
            0,
            "{page}, {effect:?} half faded: the GPU arms disagree"
        );
        // Over the light glass at alpha 153 a half-faded plasma in this amber
        // lands above both ends: pixel 96068, green, 145 against 135-143. The
        // renderer before the amber does the same with only the colour
        // changed, so the amber shows the overshoot and its edits do not cause
        // it. A channel here is premultiplied; which layer lifts it is not
        // isolated.
        let slack = if page.starts_with("light glass") {
            2
        } else {
            0
        };
        let mut between = 0;
        for (i, ((&p, &w), &m)) in plain.iter().zip(&whole).zip(&a).enumerate() {
            for k in [0u32, 8, 16] {
                let ch = |v: u32| (v >> k) & 0xff;
                let (p, w, m) = (ch(p), ch(w), ch(m));
                assert!(
                    p.min(w).saturating_sub(slack) <= m && m <= p.max(w) + slack,
                    "{page}, {effect:?} half faded: pixel {i} channel {k}: {m} is not \
                     between {p} and {w}"
                );
            }
            between += usize::from(m != p && m != w);
        }
        assert!(
            between > 50,
            "{page}, {effect:?} half faded: {between} pixels part way"
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

/// **A picture placed one cell at a time is drawn whole, on all three
/// paths**: one image and a put per cell of a 40 by 7 block, each with
/// its own source rectangle and `c=1,r=1,z=-1,C=1`, the shape yazi 26.9.1
/// sends (280 placements; the store held 64 until 2026-10-18 and drew one
/// row and 24 cells of this). Each source rectangle is 3 by 3 pixels, so
/// the image fits the gate's arena, and each is fitted into its cell with
/// the aspect kept and centred, the text's letterbox (`[[source:
/// kitty-graphics]]`, "Controlling displayed image layout"). Every arm is
/// held to the pixels computed here, then the GPU arms to each other.
fn kitty_per_cell_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    const COLS: u32 = 40;
    const ROWS: u32 = 7;
    const S: u32 = 3;
    let texel = |x: u32, y: u32| 0xFF00_0000 | ((x * 2 + 9) << 16) | ((y * 11 + 7) << 8) | 90;
    let m = arms.renderer.metrics();
    let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
    let px16 = |v: u32| ImagePx::of(u16::try_from(v).unwrap());
    t.set_cell_size(px16(cw), px16(ch));
    let fw = arms.renderer.frame_size().0.value();
    let at = |x: u32, y: u32| usize::try_from(y * fw + x).unwrap();
    feed(t, b"\x1bc\x1b[?25l");
    let (before, _) = arms.draw(t);
    const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut rgba = Vec::new();
    for y in 0..ROWS * S {
        for x in 0..COLS * S {
            let [a, r, g, b] = texel(x, y).to_be_bytes();
            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }
    let mut data = Vec::new();
    for q in rgba.chunks(3) {
        let n = u32::from_be_bytes([0, q[0], q[1], q[2]]);
        for k in 0..4 {
            data.push(B64[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]);
        }
    }
    let chunks: Vec<&[u8]> = data.chunks(4096).collect();
    for (k, c) in chunks.iter().enumerate() {
        let more = u8::from(k + 1 < chunks.len());
        let keys = match k {
            0 => format!("q=2,a=t,i=9,f=32,s={},v={},m={more}", COLS * S, ROWS * S),
            _ => format!("m={more}"),
        };
        let mut cmd = format!("\x1b_G{keys};").into_bytes();
        cmd.extend_from_slice(c);
        cmd.extend_from_slice(b"\x1b\\");
        feed(t, &cmd);
    }
    for y in 0..ROWS {
        for x in 0..COLS {
            let put = format!(
                "\x1b[{};{}H\x1b_Gq=2,a=p,i=9,p={},x={},y={},w={S},h={S},c=1,r=1,z=-1,C=1\x1b\\",
                3 + y,
                5 + x,
                y * COLS + x + 1,
                x * S,
                y * S,
            );
            feed(t, put.as_bytes());
        }
    }
    assert_eq!(
        t.images().placements().count(),
        usize::try_from(COLS * ROWS).unwrap(),
        "the premise: every cell placed"
    );
    // Each cell's square: the side the shorter of the cell's two sides,
    // centred along the longer.
    let side = cw.min(ch);
    let (ox, oy) = ((cw - side) / 2, (ch - side) / 2);
    let mut want = before.clone();
    for y in 0..ROWS {
        for x in 0..COLS {
            let (x0, y0) = ((4 + x) * cw + ox, (2 + y) * ch + oy);
            for j in 0..side {
                for i in 0..side {
                    want[at(x0 + i, y0 + j)] = texel(
                        x * S + (2 * i + 1) * S / (2 * side),
                        y * S + (2 * j + 1) * S / (2 * side),
                    );
                }
            }
        }
    }
    let (a, b) = arms.draw(t);
    let cpu = cpu_frame(arms, t);
    for (arm, got) in [("instance", &a), ("cell", &b), ("cpu", &cpu)] {
        assert_eq!(
            differing(got, &want),
            0,
            "per cell: the {arm} arm is not the text's pixels"
        );
    }
    assert_eq!(differing(&a, &b), 0, "per cell: the GPU arms disagree");
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
    kitty_history_gate(arms, t);
    kitty_alternate_gate(arms, t);
    kitty_below_gate(arms, t);
    kitty_picks_gate(arms, t);
    kitty_placeholder_gate(arms, t);
    kitty_placeholder_leaving_gate(arms, t);
    kitty_relative_gate(arms, t);
    kitty_animation_gate(arms, t);
    t.set_cell_size(ImagePx::ZERO, ImagePx::ZERO);
    feed(t, b"\x1bc");
}

/// A PNG of `w` by `h` opaque RGB pixels, its zlib stream in stored
/// blocks, written here so no decoder under test writes its own input.
fn stored_png(w: usize, h: usize, px: impl Fn(usize, usize) -> [u8; 3]) -> Vec<u8> {
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = match c & 1 {
                    1 => 0xEDB8_8320 ^ (c >> 1),
                    _ => c >> 1,
                };
            }
        }
        !c
    }
    let mut raw = Vec::new();
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            raw.extend(px(x, y));
        }
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        let n = u16::try_from(b.len()).unwrap();
        z.extend(n.to_le_bytes());
        z.extend((!n).to_le_bytes());
        z.extend_from_slice(b);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    z.extend(((b << 16) | a).to_be_bytes());
    let chunk = |kind: &[u8], data: &[u8], out: &mut Vec<u8>| {
        out.extend(u32::try_from(data.len()).unwrap().to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend(crc(&body).to_be_bytes());
    };
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend(u32::try_from(w).unwrap().to_be_bytes());
    ihdr.extend(u32::try_from(h).unwrap().to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr, &mut out);
    chunk(b"IDAT", &z, &mut out);
    chunk(b"IEND", &[], &mut out);
    out
}

/// **An iTerm2 inline picture, on all three paths** (`OSC 1337 ;
/// File=inline=1:base64`, `[[source: iterm2-images]]`): placed at the
/// cursor one pixel a device pixel, so every path draws the picture's own
/// pixels over its cells; and text written over one of its cells clears
/// that cell's part alone, the rest drawn as before, the arms agreeing on
/// both frames.
fn iterm_file_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    let m = arms.renderer.metrics();
    let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
    let px16 = |v: u32| ImagePx::of(u16::try_from(v).unwrap());
    t.set_cell_size(px16(cw), px16(ch));
    let fw = usize::try_from(arms.renderer.frame_size().0.value()).unwrap();
    let (w, h) = (
        usize::try_from(3 * cw).unwrap(),
        usize::try_from(2 * ch).unwrap(),
    );
    let rgb = |x: usize, y: usize| {
        let v = |n: usize| u8::try_from(n % 256).unwrap();
        [v(x * 9 + 20), v(y * 7 + 40), v(x * 3 + y * 5)]
    };
    let png = stored_png(w, h, rgb);
    const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut cmd = b"\x1bc\x1b[?25l\x1b[3;5H\x1b]1337;File=inline=1:".to_vec();
    for q in png.chunks(3) {
        let mut b = [0u8; 3];
        b[..q.len()].copy_from_slice(q);
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        for k in 0..4 {
            match k <= q.len() {
                true => cmd.push(B64[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]),
                false => cmd.push(b'='),
            }
        }
    }
    cmd.push(0x07);
    feed(t, &cmd);
    let (a, b) = arms.draw(t);
    let places = arms.renderer.image_placements();
    assert_eq!(places.len(), 1, "the picture is on screen");
    let (x0, y0) = (
        usize::try_from(places[0].x).unwrap(),
        usize::try_from(places[0].y).unwrap(),
    );
    assert_eq!(
        (x0, y0),
        (
            usize::try_from(4 * cw).unwrap(),
            usize::try_from(2 * ch).unwrap()
        ),
        "at the cursor's cell"
    );
    let want = |x: usize, y: usize| {
        let [r, g, bl] = rgb(x, y);
        0xFF00_0000 | u32::from(r) << 16 | u32::from(g) << 8 | u32::from(bl)
    };
    let mut wrong = 0;
    for y in 0..h {
        for x in 0..w {
            if a[(y0 + y) * fw + x0 + x] != want(x, y) {
                wrong += 1;
            }
        }
    }
    assert_eq!(wrong, 0, "the instance arm draws the picture's own pixels");
    let cpu = cpu_frame(arms, t);
    assert_eq!(differing(&a, &b), 0, "the GPU arms disagree on the picture");
    assert_eq!(
        differing(&a, &cpu),
        0,
        "the CPU arm disagrees on the picture"
    );
    // `X` over the picture's second cell of its first row.
    feed(t, b"\x1b[3;6HX");
    let (a, b) = arms.draw(t);
    let (cwu, chu) = (usize::try_from(cw).unwrap(), usize::try_from(ch).unwrap());
    let (mut kept, mut cleared) = (0, 0);
    for y in 0..h {
        for x in 0..w {
            let in_cell = (cwu..2 * cwu).contains(&x) && y < chu;
            let same = a[(y0 + y) * fw + x0 + x] == want(x, y);
            match (in_cell, same) {
                (false, false) => kept += 1,
                (true, true) => cleared += 1,
                _ => {}
            }
        }
    }
    assert_eq!(kept, 0, "the rest of the picture is drawn as before");
    assert_eq!(cleared, 0, "the cell the text landed on shows none of it");
    let cpu = cpu_frame(arms, t);
    assert_eq!(differing(&a, &b), 0, "the GPU arms disagree after the text");
    assert_eq!(
        differing(&a, &cpu),
        0,
        "the CPU arm disagrees after the text"
    );
    t.set_cell_size(ImagePx::ZERO, ImagePx::ZERO);
    feed(t, b"\x1bc");
}

/// A 29 by 21 baseline 4:2:0 JPEG of baud's own scene, and the IJG's
/// `djpeg -dct int` decode of it (`[[source: ijg-jpeg-6b]]`).
const SMALL_JPEG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../baud-domain/tests/jpeg/small.jpg"
));
const SMALL_PPM: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../baud-domain/tests/jpeg/small.ppm"
));

/// **An iTerm2 JPEG on every arm** (`[[source: iterm2-images]]`, "Any
/// image format that macOS supports"): `imgcat`'s JPEG, placed at the
/// cursor one image pixel a device pixel, draws on the instance arm the
/// reference decode's own pixels, and the cell and CPU arms draw what it
/// draws. Before the decoder it drew the broken-image placeholder.
fn iterm_jpeg_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    let m = arms.renderer.metrics();
    let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
    let px16 = |v: u32| ImagePx::of(u16::try_from(v).unwrap());
    t.set_cell_size(px16(cw), px16(ch));
    let fw = usize::try_from(arms.renderer.frame_size().0.value()).unwrap();
    // "P6\n29 21\n255\n", thirteen bytes, then the samples.
    assert_eq!(&SMALL_PPM[..13], b"P6\n29 21\n255\n");
    let want: Vec<u32> = SMALL_PPM[13..]
        .chunks(3)
        .map(|p| u32::from_be_bytes([0xFF, p[0], p[1], p[2]]))
        .collect();
    let (w, h) = (29, 21);
    const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut cmd = b"\x1bc\x1b[?25l\x1b[3;5H\x1b]1337;File=inline=1:".to_vec();
    for q in SMALL_JPEG.chunks(3) {
        let mut b = [0u8; 3];
        b[..q.len()].copy_from_slice(q);
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        for k in 0..4 {
            match k <= q.len() {
                true => cmd.push(B64[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]),
                false => cmd.push(b'='),
            }
        }
    }
    cmd.push(0x07);
    feed(t, &cmd);
    let (a, b) = arms.draw(t);
    let places = arms.renderer.image_placements();
    assert_eq!(places.len(), 1, "the picture is on screen");
    let (x0, y0) = (
        usize::try_from(places[0].x).unwrap(),
        usize::try_from(places[0].y).unwrap(),
    );
    let mut wrong = 0;
    for y in 0..h {
        for x in 0..w {
            if a[(y0 + y) * fw + x0 + x] != want[y * w + x] {
                wrong += 1;
            }
        }
    }
    assert_eq!(wrong, 0, "the instance arm draws the JPEG's decoded pixels");
    let cpu = cpu_frame(arms, t);
    assert_eq!(differing(&a, &b), 0, "the GPU arms disagree on the JPEG");
    assert_eq!(differing(&a, &cpu), 0, "the CPU arm disagrees on the JPEG");
    t.set_cell_size(ImagePx::ZERO, ImagePx::ZERO);
    feed(t, b"\x1bc");
}

/// **An iTerm2 animated GIF on every arm** (`[[source: gif89a]]`): a
/// 16 by 16 GIF of two frames, white then red, each for ten hundredths,
/// looping for ever, placed at the cursor; the instance arm draws the
/// frame shown and the cell and CPU arms draw what it draws, before the
/// clock moves it and after. Before the decoder it drew the broken-image
/// placeholder.
fn iterm_gif_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    let m = arms.renderer.metrics();
    let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
    let px16 = |v: u32| ImagePx::of(u16::try_from(v).unwrap());
    t.set_cell_size(px16(cw), px16(ch));
    let fw = usize::try_from(arms.renderer.frame_size().0.value()).unwrap();
    // A two-colour global table, background 0: white, red.
    let mut g = b"GIF89a\x10\x00\x10\x00\x80\x00\x00\xff\xff\xff\xff\x00\x00".to_vec();
    g.extend(b"\x21\xff\x0bNETSCAPE2.0\x03\x01\x00\x00\x00");
    for i in 0..2u8 {
        g.extend([0x21, 0xf9, 4, 4, 10, 0, 0, 0]);
        g.extend(b"\x2c\x00\x00\x00\x00\x10\x00\x10\x00\x00\x02");
        // Clear (4) then the index, three bits each, packed right to left:
        // 256 pixels in 1536 bits, then the End of Information code.
        let mut bits: Vec<bool> = Vec::new();
        let mut code = |c: u8| (0..3).for_each(|k| bits.push(c >> k & 1 == 1));
        for _ in 0..256 {
            code(4);
            code(i);
        }
        code(5);
        let bytes: Vec<u8> = bits
            .chunks(8)
            .map(|c| c.iter().rev().fold(0u8, |b, &x| b << 1 | u8::from(x)))
            .collect();
        for c in bytes.chunks(255) {
            g.push(u8::try_from(c.len()).unwrap());
            g.extend_from_slice(c);
        }
        g.push(0);
    }
    g.push(0x3b);
    const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut cmd = b"\x1bc\x1b[?25l\x1b[3;5H\x1b]1337;File=inline=1:".to_vec();
    for q in g.chunks(3) {
        let mut b = [0u8; 3];
        b[..q.len()].copy_from_slice(q);
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        for k in 0..4 {
            match k <= q.len() {
                true => cmd.push(B64[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]),
                false => cmd.push(b'='),
            }
        }
    }
    cmd.push(0x07);
    feed(t, &cmd);
    let start = HostTime::of_nanos(1_000_000_000);
    for (name, tick, colour) in [
        ("the first frame", None, 0xFFFF_FFFFu32),
        ("the second, its gap past", Some(100), 0xFFFF_0000),
        ("the first again", Some(200), 0xFFFF_FFFF),
    ] {
        if let Some(ms) = tick {
            let then = start.plus(std::time::Duration::from_millis(ms));
            assert_eq!(
                t.animate_images(then, ImageMotion::Play),
                Animated::Changed,
                "{name}: the premise, the tick changed the frame"
            );
        } else {
            assert_eq!(t.animate_images(start, ImageMotion::Play), Animated::Same);
        }
        let (a, b) = arms.draw(t);
        let places = arms.renderer.image_placements();
        assert_eq!(places.len(), 1, "{name}: the picture is on screen");
        let (x0, y0) = (
            usize::try_from(places[0].x).unwrap(),
            usize::try_from(places[0].y).unwrap(),
        );
        let wrong = (0..16)
            .flat_map(|y| (0..16).map(move |x| (y0 + y) * fw + x0 + x))
            .filter(|&i| a[i] != colour)
            .count();
        assert_eq!(wrong, 0, "{name}: the instance arm draws the frame shown");
        let cpu = cpu_frame(arms, t);
        assert_eq!(
            differing(&a, &b),
            0,
            "{name}: the GPU arms disagree on the GIF"
        );
        assert_eq!(
            differing(&a, &cpu),
            0,
            "{name}: the CPU arm disagrees on the GIF"
        );
    }
    t.set_cell_size(ImagePx::ZERO, ImagePx::ZERO);
    feed(t, b"\x1bc");
}

/// **A tall picture keeps its rows in the history until its last row
/// leaves, on all three paths**: images "must be scrolled along with
/// text" (`[[source: kitty-graphics]]`, "Interaction with other terminal
/// actions"), and kitty drops a placement only when its last row passes
/// the history's far end (`[[source: kitty-graphics-c]]`,
/// `scroll_filter_func`). A four-row placement is scrolled until the
/// oldest retained line is its third row, and the window looked back to
/// that line: its third and fourth rows draw over the two oldest lines,
/// and nothing of its first two. Expected pixels are the frame with the
/// placement deleted, the texels written where the text puts them.
fn kitty_history_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    let m = arms.renderer.metrics();
    let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
    let (fw, fh) = arms.renderer.frame_size();
    let (fw, fh) = (fw.value(), fh.value());
    let at = |x: u32, y: u32| usize::try_from(y * fw + x).unwrap();
    feed(t, b"\x1bc\x1b[?25l");
    for i in 0..30 {
        feed(
            t,
            format!("{i:02} the history under a picture\r\n").as_bytes(),
        );
    }
    kitty_put(t, 1, kitty_texel, &["C=1,r=4"]);
    let anchor = t
        .images()
        .placements()
        .map(|(_, p)| p.line.value())
        .next()
        .expect("the picture was placed");
    let mut i = 30;
    let oldest = |t: &Term<'_>| {
        t.image_origin(Rows::ZERO).top.value() - u64::try_from(t.scrollback().len()).unwrap()
    };
    feed(t, format!("\x1b[{};1H", WS.ws_row).as_bytes());
    while oldest(t) < anchor + 2 {
        feed(
            t,
            format!("\r\n{i:02} the history under a picture").as_bytes(),
        );
        i += 1;
    }
    assert_eq!(
        oldest(t),
        anchor + 2,
        "the premise: two of its rows retained"
    );
    assert_eq!(t.scrollback().len(), 12, "the premise: the history is full");
    arms.back = Rows::of_count(t.scrollback().len());
    let (a, b) = arms.draw(t);
    let cpu = cpu_frame(arms, t);
    let kept = t.images().placements().count();
    feed(t, b"\x1b_Ga=d,d=i,i=1,q=2\x1b\\");
    let (without, _) = arms.draw(t);
    arms.back = Rows::ZERO;
    let [[dx, dy, dw, dh], [sx, sy, sw, sh]] = kitty_expect("r=4", cw, ch);
    let mut want = without.clone();
    let top = i64::from(dy) - 2 * i64::from(ch);
    for j in 0..dh {
        let Ok(y) = u32::try_from(top + i64::from(j)) else {
            continue;
        };
        if y >= fh {
            continue;
        }
        for i in 0..dw {
            want[at(4 * cw + dx + i, y)] = kitty_texel(
                sx + (2 * i + 1) * sw / (2 * dw),
                sy + (2 * j + 1) * sh / (2 * dh),
            );
        }
    }
    assert!(
        differing(&want, &without) > 0,
        "the premise: the retained rows hold drawn pixels"
    );
    for (arm, got) in [("instance", &a), ("cell", &b), ("cpu", &cpu)] {
        assert_eq!(
            differing(got, &want),
            0,
            "the history's far end: the {arm} arm does not draw the rows still retained"
        );
    }
    assert_eq!(kept, 1, "the picture is kept while a row of it is retained");
}

/// **A picture a reverse scroll pushes below the alternate screen is kept
/// and comes back, on all three paths**: kitty drops a placement only once
/// its last row is above the screen's top, and never for being below it
/// (`[[source: kitty-graphics-c]]` `scroll_filter_func`,
/// `[[source: kitty-screen-c]]` `INDEX_GRAPHICS`). A two-row picture on
/// the alternate screen's last two rows: one `RI` at the top leaves its
/// first row on the last row and its second clipped at the frame's edge;
/// a second `RI` puts it wholly below, drawn nowhere; two line feeds at
/// the bottom bring it back on the last two rows. Each expected frame is
/// the same scene with the placement deleted, the texels written where
/// the text puts them. The cell arm reads the window's copy, as the app's
/// does on the alternate screen.
fn kitty_alternate_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    let reads = core::mem::replace(&mut arms.reads, Reads::Copy);
    let m = arms.renderer.metrics();
    let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
    let (fw, fh) = arms.renderer.frame_size();
    let (fw, fh) = (fw.value(), fh.value());
    let at = |x: u32, y: u32| usize::try_from(y * fw + x).unwrap();
    let rows = u32::from(WS.ws_row);
    assert_eq!(fh, rows * ch, "the premise: the frame ends at the last row");
    let [[dx, dy, dw, dh], [sx, sy, sw, sh]] = kitty_expect("r=2", cw, ch);
    // The scene up to step `k`, from a reset.
    let steps: [&[u8]; 3] = [b"\x1b[1;1H\x1bM", b"\x1b[1;1H\x1bM", b"\x1b[10;1H\n\n"];
    let scene = |t: &mut Term<'_>, k: usize| {
        feed(t, b"\x1bc\x1b[?25l\x1b[?1049h");
        for i in 1..=rows {
            feed(
                t,
                format!("\x1b[{i};40Hrow {i:02} of the editor").as_bytes(),
            );
        }
        kitty_put(t, 1, kitty_texel, &[]);
        feed(t, b"\x1b[9;5H\x1b_Ga=p,i=1,q=2,C=1,r=2\x1b\\");
        for step in &steps[..k] {
            feed(t, step);
        }
    };
    // The screen row the picture's anchor is on after each step.
    let anchors: [u32; 4] = [8, 9, 10, 8];
    for (k, row) in anchors.into_iter().enumerate() {
        scene(t, k);
        let (a, b) = arms.draw(t);
        let cpu = cpu_frame(arms, t);
        let kept = t.images().placements().count();
        feed(t, b"\x1b_Ga=d,d=i,i=1,q=2\x1b\\");
        let (without, _) = arms.draw(t);
        let mut want = without.clone();
        for j in 0..dh {
            let y = row * ch + dy + j;
            if y >= fh {
                continue;
            }
            for i in 0..dw {
                want[at(4 * cw + dx + i, y)] = kitty_texel(
                    sx + (2 * i + 1) * sw / (2 * dw),
                    sy + (2 * j + 1) * sh / (2 * dh),
                );
            }
        }
        assert_eq!(kept, 1, "step {k}: the picture is kept");
        assert_eq!(
            differing(&want, &without) > 0,
            row < rows,
            "step {k}: the premise: drawn exactly when on the screen"
        );
        for (arm, got) in [("instance", &a), ("cell", &b), ("cpu", &cpu)] {
            assert_eq!(
                differing(got, &want),
                0,
                "step {k}: the {arm} arm does not draw the picture where its rows are"
            );
        }
    }
    arms.reads = reads;
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
/// in all four under the ink. Then the two forks: a block cursor on the
/// third cell and the selection on the fourth draw over the image. Every
/// expected pixel is the frame without the image or the texel the text puts
/// there, never another arm's.
fn kitty_below_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    let m = arms.renderer.metrics();
    let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
    let fw = arms.renderer.frame_size().0.value();
    let at = |x: u32, y: u32| usize::try_from(y * fw + x).unwrap();
    let (ax, ay) = (4 * cw, 2 * ch);
    let scene = "\x1bc\x1b[?25l\x1b[3;5H\x1b[41m\u{2580} \x1b[0m\u{2580} ";
    let lit = Highlight::new(
        (Row::from_index(2), Col::at_index(7)),
        (Row::from_index(2), Col::at_index(7)),
        Region::Stream,
    );
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

/// **The two choices for an image below `INT32_MIN/2`, on all three
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
        "/../docs/research/sources/kitty-placeholder-diacritics.txt"
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

/// `a=f` of a 6 by 4 `f=32` frame of image `id`, `px`'s pixels.
fn kitty_frame(t: &mut Term<'_>, id: u32, px: fn(u32, u32) -> u32) {
    const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut rgba = Vec::new();
    for y in 0..4 {
        for x in 0..6 {
            let [a, r, g, b] = px(x, y).to_be_bytes();
            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }
    let mut cmd = format!("\x1b_Ga=f,f=32,s=6,v=4,i={id},q=2;").into_bytes();
    for q in rgba.chunks(3) {
        let n = u32::from_be_bytes([0, q[0], q[1], q[2]]);
        for k in 0..4 {
            cmd.push(B64[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]);
        }
    }
    cmd.extend_from_slice(b"\x1b\\");
    feed(t, &cmd);
}

/// **A kitty animation's frame on every arm** (`[[source:
/// kitty-graphics]]`, "Animation"): an image with a second frame draws, on
/// the instance, cell and CPU arms alike, what a still image of the frame
/// it shows draws -- its root until a client's `a=a,c=2` makes the second
/// current, then the second, then the root again when the clock's tick
/// passes the second's gap. The frame changes at one store epoch, so a
/// texture kept by the image alone would draw the frame before.
fn kitty_animation_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    let frames = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>| {
        let (a, b) = arms.draw(t);
        let cpu = cpu_frame(arms, t);
        [a, b, cpu]
    };
    feed(t, b"\x1bc\x1b[?25l");
    kitty_put(t, 1, kitty_texel, &["C=1"]);
    let root = frames(arms, t);
    feed(t, b"\x1bc\x1b[?25l");
    kitty_put(t, 2, kitty_other, &["C=1"]);
    let second = frames(arms, t);
    assert!(
        differing(&root[0], &second[0]) > 0,
        "the premise: the two frames draw apart"
    );
    feed(t, b"\x1bc\x1b[?25l");
    kitty_put(t, 1, kitty_texel, &["C=1"]);
    kitty_frame(t, 1, kitty_other);
    let start = HostTime::of_nanos(1_000_000_000);
    // (name, the command, a tick's milliseconds, the frames expected).
    type Step<'s> = (&'s str, &'s [u8], Option<u64>, &'s [Vec<u32>; 3]);
    let steps: [Step<'_>; 3] = [
        ("the root, before any control", b"", None, &root),
        (
            "the second, made current",
            b"\x1b_Ga=a,i=1,c=2\x1b\\",
            None,
            &second,
        ),
        (
            "the root again, its gap past",
            b"\x1b_Ga=a,i=1,r=1,z=40,s=3\x1b\\",
            Some(40),
            &root,
        ),
    ];
    for (name, cmd, tick, want) in steps {
        feed(t, cmd);
        if let Some(ms) = tick {
            assert_eq!(t.animate_images(start, ImageMotion::Play), Animated::Same);
            let then = start.plus(std::time::Duration::from_millis(ms));
            assert_eq!(
                t.animate_images(then, ImageMotion::Play),
                Animated::Changed,
                "{name}: the premise, the tick changed the frame"
            );
        }
        let got = frames(arms, t);
        for (k, arm) in ["instance", "cell", "cpu"].into_iter().enumerate() {
            assert_eq!(
                differing(&got[k], &want[k]),
                0,
                "{name}: the {arm} arm does not draw the frame shown"
            );
        }
    }
}

/// **mpv's kitty video draws alike on every arm, and as mpv sent it**:
/// `tests/recordings/kitty/mpv-logo-40x6.raw`, mpv 0.41.0 `--vo=kitty`
/// playing the README's logo, fed a read at a time, each picture's decode
/// run out. At the end of each of its thirteen uploads the instance, cell
/// and CPU arms draw the same frame, and the instance arm's pixels where
/// the frames are placed are the RGB of the newest complete upload,
/// decoded from the capture's chunks by `mpv_capture`, which shares
/// nothing with the decoder: so the three agree on mpv's picture and not
/// on a blank. The controls before any comparison: nothing was refused but
/// each cut upload, short, and nothing cancelled -- the status line inside
/// the chunk is its payload, kitty's reading; the frames pile up, one
/// placement an upload, every one at the cell mpv put them in and drawn;
/// and a frame unlike the first is drawn over it. At a cut upload the
/// frame before stays on top. The cell arm reads the window's copy, as the
/// app's does on the alternate screen.
///
/// The budget holds every frame and the one a cut upload reserves, as the
/// app's does at this size, so the quota deletes none;
/// `under_a_two_frame_quota_mpvs_video_draws_alike_on_every_arm` holds the
/// arms where it does.
///
/// No pixel brighter than luminance 200 in the source draws at or below
/// it on the instance arm, checked before the exact RGB: a faint picture
/// is the symptom it names.
///
/// Perturbations, each red here: `f=24` stored with alpha 0 -- the
/// bright pixels of the first upload draw faint, the page under them; the
/// draw order reversed -- the oldest frame is on top at the third upload,
/// the first unlike the first frame.
#[test]
fn mpvs_kitty_video_draws_alike_on_every_arm() {
    let bytes = std::fs::read(mpv_capture::PATH).expect("the capture");
    let ups = mpv_capture::uploads(&bytes);
    let (mpv_capture::Pixels(s), mpv_capture::Pixels(v)) = ups[0].size;
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
    let frames = ups.iter().filter(|u| u.rgb.is_some()).count();
    // Every frame, and the one a cut upload reserves before it comes up
    // short.
    arms!(renderer => arms, t, ImageBudget::of((frames + 1) * s * v + 64));
    // The glyph map in use before the first frame, whose screen is blank;
    // and the cell arm reading the window's copy, as the app's does on the
    // alternate screen, where mpv draws every frame.
    arms.warm(&mut t);
    feed(&mut t, b"\x1bc");
    arms.reads = Reads::Copy;
    let m = arms.renderer.metrics();
    let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
    let px16 = |v: u32| ImagePx::of(u16::try_from(v).unwrap());
    t.set_cell_size(px16(cw), px16(ch));
    let fw = usize::try_from(arms.renderer.frame_size().0.value()).unwrap();
    let (mut fed, mut complete, mut cuts, mut over_first) = (0usize, 0usize, 0usize, 0usize);
    let mut bright = 0usize;
    let mut shown: Option<&[u8]> = None;
    let mut first: Option<&[u8]> = None;
    let mut origin: Option<(i16, i16)> = None;
    for (k, up) in ups.iter().enumerate() {
        while fed < up.end.0 {
            let end = (fed + 1024).min(up.end.0);
            feed(&mut t, &bytes[fed..end]);
            while t.decode_step(baud_domain::DecodeBudget::MIB) == baud_domain::DecodeProgress::More
            {
            }
            fed = end;
        }
        cuts += usize::from(up.cut.is_some());
        assert_eq!(
            (t.kitty().refused, t.kitty().cancelled),
            (cuts, 0),
            "upload {k}: refused but each cut upload, short, and nothing cancelled"
        );
        if let Some(rgb) = &up.rgb {
            complete += 1;
            if first.is_some_and(|f| f != rgb.as_slice()) {
                over_first += 1;
            }
            first.get_or_insert(rgb.as_slice());
            shown = Some(rgb.as_slice());
        }
        let (a, b) = arms.draw(&mut t);
        let places = arms.renderer.image_placements().to_vec();
        let cpu = cpu_frame(&mut arms, &mut t);
        let placed = t.images().placements().count();
        assert_eq!(
            placed + t.images().placements_expired,
            complete,
            "upload {k}: one placement an upload, deleted by nothing but the scroll"
        );
        assert_eq!(places.len(), placed, "upload {k}: every placement drawn");
        if let (Some(p), Some(rgb)) = (places.first(), shown) {
            let at = (p.x, p.y);
            for q in &places {
                assert_eq!(
                    ((q.x, q.y), usize::from(q.w), usize::from(q.h)),
                    (at, s, v),
                    "upload {k}: every frame at one cell, whole"
                );
            }
            assert_eq!(
                *origin.get_or_insert(at),
                at,
                "upload {k}: the frames' cell"
            );
            let (x0, y0) = (
                usize::try_from(at.0).unwrap(),
                usize::try_from(at.1).unwrap(),
            );
            let (lit, dim) = mpv_capture::faint(&a, mpv_capture::Pixels(fw), (x0, y0), (s, v), rgb);
            bright += lit;
            assert_eq!(
                dim, 0,
                "upload {k}: of {lit} bright pixels, the instance arm draws {dim} faint"
            );
            let wrong = (0..v)
                .flat_map(|y| (0..s).map(move |x| (x, y)))
                .filter(|&(x, y)| {
                    let o = 3 * (y * s + x);
                    let want = 0xFF00_0000
                        | u32::from(rgb[o]) << 16
                        | u32::from(rgb[o + 1]) << 8
                        | u32::from(rgb[o + 2]);
                    a[(y0 + y) * fw + x0 + x] != want
                })
                .count();
            assert_eq!(
                wrong, 0,
                "upload {k}: the instance arm is not the newest frame's RGB"
            );
        }
        assert_eq!(differing(&a, &b), 0, "upload {k}: the GPU arms disagree");
        assert_eq!(differing(&a, &cpu), 0, "upload {k}: the CPU arm disagrees");
    }
    assert_eq!(complete, frames);
    assert!(
        over_first > 0,
        "the premise: a frame unlike the first is drawn on it"
    );
    assert!(origin.is_some(), "the premise: a frame was drawn");
    assert!(bright > 0, "the premise: the frames have bright pixels");
    assert_eq!(t.images().images_evicted, 0, "the budget holds every frame");
    t.set_cell_size(ImagePx::ZERO, ImagePx::ZERO);
}

/// **Under a quota of two frames mpv's video draws alike on every arm,
/// each upload its own frame**: the capture of
/// `mpvs_kitty_video_draws_alike_on_every_arm` with a budget of two of its
/// frames, so each new frame from the third deletes the oldest for space
/// and its placement takes the freed slot. Frames that tie on `z` with no
/// id draw in the order their images were stored, as kitty's render sort
/// orders them (`[[source: kitty-graphics-c]]`, `lt`: `z_index`, then
/// `image_id`, then `ref_id`), on the instance, cell and CPU arms alike,
/// and the instance arm's region is the newest complete upload's RGB.
///
/// The break: today's slot order -- the frame placed into the freed low
/// slot goes under the one before it, red at the third upload.
#[test]
fn under_a_two_frame_quota_mpvs_video_draws_alike_on_every_arm() {
    let bytes = std::fs::read(mpv_capture::PATH).expect("the capture");
    let ups = mpv_capture::uploads(&bytes);
    let (mpv_capture::Pixels(s), mpv_capture::Pixels(v)) = ups[0].size;
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
    arms!(renderer => arms, t, ImageBudget::of(2 * s * v + 64));
    arms.warm(&mut t);
    feed(&mut t, b"\x1bc");
    arms.reads = Reads::Copy;
    let m = arms.renderer.metrics();
    let px16 = |v: u32| ImagePx::of(u16::try_from(v).unwrap());
    t.set_cell_size(px16(m.cell_w.value()), px16(m.cell_h.value()));
    let fw = usize::try_from(arms.renderer.frame_size().0.value()).unwrap();
    let (mut fed, mut drawn) = (0usize, 0usize);
    let mut shown: Option<&[u8]> = None;
    // The complete uploads whose frame is the one before's, which a wrong
    // order could not be told from.
    let mut repeats: Vec<usize> = Vec::new();
    for (k, up) in ups.iter().enumerate() {
        while fed < up.end.0 {
            let end = (fed + 1024).min(up.end.0);
            feed(&mut t, &bytes[fed..end]);
            while t.decode_step(baud_domain::DecodeBudget::MIB) == baud_domain::DecodeProgress::More
            {
            }
            fed = end;
        }
        if let Some(rgb) = &up.rgb {
            if shown == Some(rgb.as_slice()) {
                repeats.push(k);
            }
            shown = Some(rgb.as_slice());
        }
        let (a, b) = arms.draw(&mut t);
        let places = arms.renderer.image_placements().to_vec();
        let cpu = cpu_frame(&mut arms, &mut t);
        if let (Some(p), Some(rgb)) = (places.first(), shown) {
            let (x0, y0) = (usize::try_from(p.x).unwrap(), usize::try_from(p.y).unwrap());
            let wrong = (0..v)
                .flat_map(|y| (0..s).map(move |x| (x, y)))
                .filter(|&(x, y)| {
                    let o = 3 * (y * s + x);
                    let want = 0xFF00_0000
                        | u32::from(rgb[o]) << 16
                        | u32::from(rgb[o + 1]) << 8
                        | u32::from(rgb[o + 2]);
                    a[(y0 + y) * fw + x0 + x] != want
                })
                .count();
            assert_eq!(
                wrong, 0,
                "upload {k}: the instance arm is not the newest frame's RGB"
            );
            drawn += 1;
        }
        assert_eq!(differing(&a, &b), 0, "upload {k}: the GPU arms disagree");
        assert_eq!(differing(&a, &cpu), 0, "upload {k}: the CPU arm disagrees");
    }
    assert!(
        t.images().images_evicted >= 8,
        "the premise: the quota deleted frames, {} of them",
        t.images().images_evicted
    );
    // Every upload: the last one's status line no longer prints and
    // scrolls the frames away, kitty's reading of a chunk it cuts.
    assert_eq!(drawn, 13, "the premise: every upload drawn");
    assert_eq!(
        repeats,
        vec![1],
        "the premise: every frame differs from the one before but the second"
    );
    t.set_cell_size(ImagePx::ZERO, ImagePx::ZERO);
}

/// **Placements that tie on `z` stack as the text says, and where it is
/// silent as kitty does, on every arm and on the CPU path's incremental
/// frame.** Two cases at one cell, each held on the instance and cell
/// arms, a whole CPU frame and an incremental CPU frame kept from the frame
/// before:
///
/// - **ids out of the order the images were made**: image 5 stored and
///   placed first, image 3 after it, draws 5 over 3 -- "If two images with
///   the same z-index overlap then the image with the lower id is
///   considered to have the lower z-index" (`[[source: kitty-graphics]]`).
///   kitty itself orders by the id it makes each image with, and would draw
///   3 over 5 (`[[source: kitty-graphics-c]]`, `lt`); baud follows the
///   text. Break: drop the client's id from the sort -- red.
/// - **a placement deleted and put again** in one frame: image 1's `p=1`,
///   its left half, under `p=2`, its right half, both at one cell and at
///   `z=-1`, under the text, where the CPU path blits a picture only in the
///   rows it repaints; `p=1`
///   deleted and put back where it was is the newer placement and draws
///   over `p=2`, as kitty's `ref_id` orders placements of one image. It
///   takes the slot it left, below `p=2`'s, so by slot it would stay under.
///   Breaks: drop the placement's serial from the sort -- red on every arm;
///   leave it and the image's age out of the placement's key -- the
///   incremental CPU frame finds no placement changed, keeps `p=2` on top,
///   and is red alone.
#[test]
fn placements_that_tie_on_z_stack_alike_on_every_arm() {
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
    arms!(renderer => arms, t);
    arms.warm(&mut t);
    let m = arms.renderer.metrics();
    let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
    let px16 = |v: u32| ImagePx::of(u16::try_from(v).unwrap());
    t.set_cell_size(px16(cw), px16(ch));
    let (fw, fh) = arms.renderer.whole_size();
    let at = usize::try_from(2 * ch * fw.value() + 4 * cw).unwrap();
    let mut kept = vec![0u32; usize::try_from(fw.value() * fh.value()).unwrap()];
    // A CPU frame into `kept`, whole or from the one before.
    let mut cpu_into = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, damage: Damage| {
        let mut target = Target::new(&mut kept, fw, fw, fh).expect("target");
        arms.renderer.render_at(t, arms.back, &mut target, damage);
        kept.iter()
            .map(|&p| premultiply(Argb(p)).0)
            .collect::<Vec<u32>>()
    };
    // (name, the state before, the change, the top before, the top after,
    // the top a wrong order draws).
    type Case<'c> = (&'c str, fn(&mut Term<'_>), fn(&mut Term<'_>), u32, u32, u32);
    let cases: [Case<'_>; 2] = [
        (
            "ids 5 then 3",
            |t| {
                feed(t, b"\x1bc\x1b[?25l");
                kitty_put(t, 5, kitty_texel, &["C=1"]);
            },
            |t| kitty_put(t, 3, kitty_other, &["C=1"]),
            kitty_texel(0, 0),
            kitty_texel(0, 0),
            kitty_other(0, 0),
        ),
        (
            "p=1 put again over p=2",
            |t| {
                feed(t, b"\x1bc\x1b[?25l");
                kitty_put(
                    t,
                    1,
                    kitty_texel,
                    &[
                        "p=1,x=0,y=0,w=3,h=4,C=1,z=-1",
                        "p=2,x=3,y=0,w=3,h=4,C=1,z=-1",
                    ],
                );
            },
            |t| {
                feed(t, b"\x1b_Ga=d,d=i,i=1,p=1,q=2\x1b\\");
                feed(
                    t,
                    b"\x1b[3;5H\x1b_Ga=p,i=1,p=1,x=0,y=0,w=3,h=4,C=1,z=-1,q=2\x1b\\",
                );
            },
            kitty_texel(3, 0),
            kitty_texel(0, 0),
            kitty_texel(3, 0),
        ),
    ];
    for (name, before, change, first, over, wrong) in cases {
        assert_ne!(over, wrong, "{name}: the premise, the two draw apart");
        before(&mut t);
        let was = cpu_into(&mut arms, &mut t, Damage::Full);
        assert_eq!(was[at], first, "{name}: the premise, the stack before");
        change(&mut t);
        let incremental = cpu_into(&mut arms, &mut t, Damage::Incremental);
        let (a, b) = arms.draw(&mut t);
        let cpu = cpu_frame(&mut arms, &mut t);
        for (arm, got) in [
            ("instance", &a),
            ("cell", &b),
            ("cpu", &cpu),
            ("incremental cpu", &incremental),
        ] {
            assert_eq!(got[at], over, "{name}: the {arm} arm's top");
        }
        assert_eq!(differing(&a, &b), 0, "{name}: the GPU arms disagree");
        assert_eq!(differing(&a, &cpu), 0, "{name}: the CPU arm disagrees");
        assert_eq!(
            differing(&cpu, &incremental),
            0,
            "{name}: the incremental frame is not the whole one"
        );
    }
    t.set_cell_size(ImagePx::ZERO, ImagePx::ZERO);
}

/// **kitty's relative placements on every arm** (`[[source:
/// kitty-graphics]]`, "Relative placements"): a placement made with `P`,
/// `Q`, `H` and `V` draws, on the instance, cell and CPU arms alike, what
/// the same image placed at its parent's top-left cell plus `H` columns
/// and `V` rows draws -- under a parent on screen; under a virtual parent,
/// at the least row and the least column of the placeholders showing it
/// ("The x position is the minimum of all the placeholder x positions and
/// the y position is the minimum of all the placeholder y positions"), so
/// a grid begun a cell in shifts it; and left of column zero it hangs over
/// the edge, the part on screen drawn ("Negative values move left").
fn kitty_relative_gate(arms: &mut Arms<'_, '_>, t: &mut Term<'_>) {
    let m = arms.renderer.metrics();
    let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
    let fw = arms.renderer.frame_size().0.value();
    let at = |x: u32, y: u32| usize::try_from(y * fw + x).unwrap();
    let frames = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>| {
        let (a, b) = arms.draw(t);
        let cpu = cpu_frame(arms, t);
        [a, b, cpu]
    };
    let all = |c: usize, r: usize| -> Vec<(usize, usize)> {
        (0..r).flat_map(|y| (0..c).map(move |x| (y, x))).collect()
    };
    feed(t, b"\x1bc\x1b[?25l");
    let blank = frames(arms, t);
    // (name, the scene with the relative placement, the same pixels
    // placed directly).
    let put2 = |row: u32, col: u32| format!("\x1b[{row};{col}H\x1b_Ga=p,i=2,C=1,q=2\x1b\\");
    let rel2 = |keys: &str| format!("\x1b[20;1H\x1b_Ga=p,i=2,q=2,{keys}\x1b\\");
    type Scene<'s> = (&'s str, &'s [&'s str], Vec<u8>, Vec<u8>, Vec<u8>);
    let scenes: [Scene<'_>; 3] = [
        (
            "under a parent on screen",
            &["C=1,p=1"],
            rel2("P=1,Q=1,H=8,V=2").into_bytes(),
            Vec::new(),
            put2(5, 13).into_bytes(),
        ),
        (
            "under a virtual parent",
            &["U=1,p=4,c=4,r=3"],
            rel2("P=1,Q=4,H=6,V=1").into_bytes(),
            placeholder_grid("38;5;1;58;5;4", &all(4, 3), 0, true),
            put2(4, 11).into_bytes(),
        ),
        (
            "under a virtual parent, a cell in",
            &["U=1,p=4,c=4,r=3"],
            rel2("P=1,Q=4,H=6,V=1").into_bytes(),
            placeholder_grid("38;5;1;58;5;4", &[(1, 1), (1, 2), (2, 3)], 0, true),
            put2(5, 12).into_bytes(),
        ),
    ];
    for (name, parent, rel, grid, direct) in scenes {
        feed(t, b"\x1bc\x1b[?25l");
        kitty_put(t, 1, kitty_texel, parent);
        kitty_put(t, 2, kitty_other, &[]);
        feed(t, &grid);
        feed(t, &direct);
        let want = frames(arms, t);
        assert!(
            differing(&want[0], &blank[0]) > 0,
            "{name}: the premise, the direct placement draws"
        );
        feed(t, b"\x1bc\x1b[?25l");
        kitty_put(t, 1, kitty_texel, parent);
        kitty_put(t, 2, kitty_other, &[]);
        feed(t, &grid);
        feed(t, &rel);
        let got = frames(arms, t);
        for (k, arm) in ["instance", "cell", "cpu"].into_iter().enumerate() {
            assert_eq!(
                differing(&got[k], &want[k]),
                0,
                "{name}: the {arm} arm's relative placement is not where its parent puts it"
            );
        }
    }
    // Left of column zero: a parent in the first column and `H=-1` puts the
    // child's box, two cells wide, half over the edge; the half on screen
    // is the box's right half, the text's pixels.
    feed(t, b"\x1bc\x1b[?25l");
    let (before, _) = arms.draw(t);
    kitty_put(t, 2, kitty_other, &[]);
    feed(t, b"\x1b[3;1H\x1b_Ga=t,f=24,s=1,v=1,i=1,q=2;AAAA\x1b\\");
    feed(t, b"\x1b_Ga=p,i=1,p=1,C=1,q=2\x1b\\");
    feed(t, rel2("P=1,Q=1,H=-1,c=2").as_bytes());
    let got = frames(arms, t);
    let [[dx, dy, dw, dh], [sx, sy, sw, sh]] = kitty_expect("c=2", cw, ch);
    let mut want = before;
    // The parent, one black pixel at the anchor.
    want[at(0, 2 * ch)] = 0xFF00_0000;
    for j in 0..dh {
        for i in cw..dw {
            want[at(dx + i - cw, 2 * ch + dy + j)] = kitty_other(
                sx + (2 * i + 1) * sw / (2 * dw),
                sy + (2 * j + 1) * sh / (2 * dh),
            );
        }
    }
    for (k, arm) in ["instance", "cell", "cpu"].into_iter().enumerate() {
        assert_eq!(
            differing(&got[k], &want),
            0,
            "over the edge: the {arm} arm is not the text's pixels"
        );
    }
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
    // **On sized lines**: a 4x2 grid whose first row is on a double-width
    // line and whose second is on the top half of a double-height pair,
    // the pair's bottom half writing that row again, drawn as characters
    // there (`[[source: dec-std-070]]` § 5.6.1), alike on every arm and
    // unlike the same grid on single-width lines; and a Sixel begun on a
    // double-width line, at the cursor's displayed place
    // (UNSCALED_AT_CURSOR).
    let sized = |set: [&str; 3]| {
        let mut g =
            format!("\x1b[3;1H{}\x1b[4;1H{}\x1b[5;1H{}", set[0], set[1], set[2]).into_bytes();
        g.extend(placeholder_grid("38;5;1", &all(4, 2), 0, true));
        g.extend(placeholder_grid(
            "38;5;1",
            &[(2, 0), (2, 1), (2, 2), (2, 3)],
            0,
            false,
        ));
        g
    };
    let mut got = Vec::new();
    for set in [["", "", ""], ["\x1b#6", "\x1b#3", "\x1b#4"]] {
        feed(t, b"\x1bc\x1b[?25l");
        kitty_put(t, 1, kitty_texel, &["U=1,c=4,r=3"]);
        feed(t, &sized(set));
        got.push(frames(arms, t));
    }
    let [plain, doubled] = [&got[0], &got[1]];
    assert!(
        differing(&plain[0], &doubled[0]) > 0,
        "the sized grid is drawn otherwise"
    );
    for (k, arm) in ["cell", "cpu"].into_iter().enumerate() {
        assert_eq!(
            differing(&doubled[0], &doubled[k + 1]),
            0,
            "the {arm} arm disagrees on placeholders on sized lines"
        );
    }
    let sixel = b"\x1bP0;1;0q#0;2;100;0;0!24~-!24~\x1b\\";
    feed(t, b"\x1bc\x1b[?25l\x1b[3;1H\x1b#6Doubled text\x1b[3;9H");
    feed(t, sixel);
    let [a, b, cpu] = frames(arms, t);
    assert_eq!(
        differing(&a, &b),
        0,
        "the GPU arms disagree on a Sixel on a double line"
    );
    assert_eq!(
        differing(&a, &cpu),
        0,
        "the CPU arm disagrees on a Sixel on a double line"
    );
    // **Moved by IL, DL, DECIC and DECDC**: IL and DL carry the line, its
    // rendition and the Sixel on it (`[[source: dec-std-070]]` pp. 5-146 to
    // 5-149), so every arm draws what the same line and Sixel put straight
    // on the row they reach draws; DECIC and DECDC shift the text under a
    // Sixel that stays, so every arm draws the shifted text with the Sixel
    // put where it was. A kitty placement on a sized line under the four is
    // held by the core tests alone
    // (`il_and_dl_carry_an_image_on_a_sized_line_with_its_line`,
    // `decic_and_decdc_leave_an_image_on_a_sized_line_where_it_is`).
    let put = |row: usize, text: &str| {
        let mut b = format!("\x1bc\x1b[?25l\x1b[{row};1H\x1b#6{text}\x1b[{row};9H").into_bytes();
        b.extend_from_slice(sixel);
        b
    };
    let plain = {
        feed(t, &put(5, "Doubled text"));
        frames(arms, t)[0].clone()
    };
    for (op, row, text) in [
        ("\x1b[2;1H\x1b[2L", 7, "Doubled text"),
        ("\x1b[2;1H\x1b[2M", 3, "Doubled text"),
        ("\x1b[5;1H\x1b[2'}", 5, "  Doubled text"),
        ("\x1b[5;1H\x1b[2'~", 5, "ubled text"),
    ] {
        feed(t, &put(row, text));
        let want = frames(arms, t)[0].clone();
        feed(t, &put(5, "Doubled text"));
        feed(t, op.as_bytes());
        let got = frames(arms, t);
        assert!(differing(&plain, &got[0]) > 0, "{op:?} moved nothing");
        for (k, arm) in ["instance", "cell", "cpu"].into_iter().enumerate() {
            assert_eq!(
                differing(&want, &got[k]),
                0,
                "{op:?}: the {arm} arm is not the line and Sixel put on row {row}"
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

/// The precomposed letters of the Latin, Greek and Cyrillic blocks: every
/// alphabetic one-column code point from U+00C0 to U+024F, U+0386 to U+04FF
/// and U+1E00 to U+1FFF, whether or not a face maps it.
fn precomposed_letters() -> Vec<char> {
    [
        0x00c0..=0x024f,
        0x0386..=0x03ff,
        0x0400..=0x04ff,
        0x1e00..=0x1eff,
        0x1f00..=0x1fff,
    ]
    .into_iter()
    .flatten()
    .filter_map(char::from_u32)
    .filter(|c| c.is_alphabetic())
    .filter(|&c| {
        let cp = baud_domain::Codepoint::new(u32::from(c)).unwrap();
        baud_domain::Classified::of(cp).width(baud_domain::WidthPolicy::DEFAULT)
            == baud_domain::Width::NARROW
    })
    .collect()
}

/// Every letter of `letters`, in rendition `sgr`, drawn on all three arms:
/// one a cell, two empty columns and two empty rows between them, so each
/// letter has the cells around it to itself -- a stacked accent the row
/// above, a hook the row below. The letters whose pixels differ between the arms: each with
/// the count on the cell arm and on the CPU arm, and how many glyphs it
/// shapes to alone.
fn letters_gate<'f, 'a>(
    fresh: &dyn Fn() -> Renderer<'f, 'a>,
    letters: &[char],
    sgr: &str,
    style: FaceStyle,
) -> Vec<(char, usize, usize, usize)> {
    let (pw, m) = {
        let r = fresh();
        (r.frame_size().0, r.metrics())
    };
    let w = usize::try_from(pw.value()).unwrap();
    let (cw, ch) = (
        usize::try_from(m.cell_w.value()).unwrap(),
        usize::try_from(m.cell_h.value()).unwrap(),
    );
    let (cols, rows) = (usize::from(WS.ws_col), usize::from(WS.ws_row));
    let per_row = cols / 3;
    let per_screen = per_row * (rows / 3);
    // The screen with nothing on it, drawn once the map holds the first
    // screen's letters (`Arms::draw` asserts the map is in use).
    let mut empty: Option<Vec<u32>> = None;
    let mut split = Vec::new();
    let mut inked = 0;
    for screen in letters.chunks(even(letters.len(), per_screen)) {
        // Each screen the first frame of a renderer of its own
        // (`first_frame`): a renderer the last screen's conversion
        // resolved the rendition for would hide a cell arm that read it.
        arms!(fresh() => arms, t);
        let (arms, t) = (&mut arms, &mut t);
        arms.warm(t);
        let mut text = format!("\x1bc\x1b[?25l{sgr}");
        let mut at = Vec::new();
        for (i, &c) in screen.iter().enumerate() {
            let (col, row) = (1 + 3 * (i % per_row), 1 + 3 * (i / per_row));
            text.push_str(&format!("\x1b[{};{}H{c}", row + 1, col + 1));
            at.push((c, col, row));
        }
        feed(t, text.as_bytes());
        let (a, b) = arms.draw(t);
        let cpu = cpu_frame(arms, t);
        let empty = match &empty {
            Some(e) => e.clone(),
            None => {
                feed(t, b"\x1bc\x1b[?25l");
                let e = arms.draw(t).0;
                empty = Some(e.clone());
                e
            }
        };
        // A letter's cell and the eight around it.
        let near = |col: usize, row: usize| {
            ((row - 1) * ch..(row + 2) * ch)
                .flat_map(move |py| ((col - 1) * cw..(col + 2) * cw).map(move |px| py * w + px))
        };
        let count =
            |x: &[u32], y: &[u32], col, row| near(col, row).filter(|&i| x[i] != y[i]).count();
        // The premise of the attribution: no letter's ink, on any arm,
        // leaves its nine cells.
        let mut mine = vec![false; a.len()];
        for &(_, col, row) in &at {
            for i in near(col, row) {
                mine[i] = true;
            }
        }
        for (arm, px) in [("instance", &a), ("cell", &b), ("CPU", &cpu)] {
            let stray = (0..a.len())
                .filter(|&i| !mine[i] && px[i] != empty[i])
                .count();
            assert_eq!(
                stray, 0,
                "{stray} pixels of the {arm} arm lie outside every letter's cells"
            );
        }
        for (c, col, row) in at {
            inked += usize::from(count(&a, &empty, col, row) > 0);
            let (gpu, soft) = (count(&a, &b, col, row), count(&a, &cpu, col, row));
            if gpu + soft != 0 {
                let cp = baud_domain::Codepoint::new(u32::from(c)).unwrap();
                let glyphs = arms
                    .renderer
                    .cluster(&[cp], style, baud_render::CellSpan::One)
                    .len();
                split.push((c, gpu, soft, glyphs));
            }
        }
    }
    assert!(
        inked > letters.len() / 2,
        "the premise: the instance arm drew most of the letters, {inked} of {}",
        letters.len()
    );
    split
}

/// **The size of the chunks `n` items fall into, at most `most` a chunk
/// and as even as that allows**, so no screen is left a handful of letters
/// -- `Arms::draw` requires the map to hold a screen's glyphs.
fn even(n: usize, most: usize) -> usize {
    let chunks = n.div_ceil(most.max(1)).max(1);
    n.div_ceil(chunks).max(1)
}

/// `c`'s full canonical decomposition in canonical order
/// (`[[source: unicode18-ch3]]` D68, D109), from `baud_font`'s tables:
/// `None` for a letter that has none.
fn canonical_decomposition(c: char) -> Option<String> {
    fn push(c: baud_domain::Codepoint, out: &mut Vec<baud_domain::Codepoint>) {
        match baud_font::decompose(c) {
            Some((a, b)) => {
                push(a, out);
                if let Some(b) = b {
                    push(b, out);
                }
            }
            None => out.push(c),
        }
    }
    let cp = baud_domain::Codepoint::new(u32::from(c)).unwrap();
    baud_font::decompose(cp)?;
    let mut out = Vec::new();
    push(cp, &mut out);
    // Marks after a starter in canonical order: a stable sort by class.
    let mut i = 1;
    while i < out.len() {
        let class = baud_font::combining_class(out[i]).0;
        let mut j = i;
        while class != 0 && j > 0 && baud_font::combining_class(out[j - 1]).0 > class {
            out.swap(j - 1, j);
            j -= 1;
        }
        i += 1;
    }
    Some(
        out.into_iter()
            .filter_map(|c| char::from_u32(c.value()))
            .collect(),
    )
}

/// Every letter of `letters` that has a canonical decomposition drawn on
/// all three arms twice, as the precomposed letter and as its
/// decomposition typed, at the places `letters_gate` uses: the letters
/// whose two forms differ on some arm, each with the pixels by which they
/// differ on the instance, cell and CPU arms.
fn forms_gate<'f, 'a>(
    fresh: &dyn Fn() -> Renderer<'f, 'a>,
    letters: &[char],
    sgr: &str,
) -> Vec<(char, String, [usize; 3])> {
    let (pw, m) = {
        let r = fresh();
        (r.frame_size().0, r.metrics())
    };
    let w = usize::try_from(pw.value()).unwrap();
    let (cw, ch) = (
        usize::try_from(m.cell_w.value()).unwrap(),
        usize::try_from(m.cell_h.value()).unwrap(),
    );
    let (cols, rows) = (usize::from(WS.ws_col), usize::from(WS.ws_row));
    let per_row = cols / 3;
    let per_screen = per_row * (rows / 3);
    let pairs: Vec<(char, String)> = letters
        .iter()
        .filter_map(|&c| canonical_decomposition(c).map(|d| (c, d)))
        .collect();
    let mut split = Vec::new();
    for screen in pairs.chunks(even(pairs.len(), per_screen)) {
        let mut frames = Vec::new();
        for decomposed in [false, true] {
            // Each form the first frame of a renderer of its own.
            arms!(fresh() => arms, t);
            let (arms, t) = (&mut arms, &mut t);
            arms.warm(t);
            let mut text = format!("\x1bc\x1b[?25l{sgr}");
            for (i, (c, d)) in screen.iter().enumerate() {
                let (col, row) = (1 + 3 * (i % per_row), 1 + 3 * (i / per_row));
                text.push_str(&format!("\x1b[{};{}H", row + 1, col + 1));
                match decomposed {
                    false => text.push(*c),
                    true => text.push_str(d),
                }
            }
            feed(t, text.as_bytes());
            let (a, b) = arms.draw(t);
            let cpu = cpu_frame(arms, t);
            frames.push([a, b, cpu]);
        }
        let near = |col: usize, row: usize| {
            ((row - 1) * ch..(row + 2) * ch)
                .flat_map(move |py| ((col - 1) * cw..(col + 2) * cw).map(move |px| py * w + px))
        };
        for (i, (c, d)) in screen.iter().enumerate() {
            let (col, row) = (1 + 3 * (i % per_row), 1 + 3 * (i / per_row));
            let n = |k: usize| {
                near(col, row)
                    .filter(|&p| frames[0][k][p] != frames[1][k][p])
                    .count()
            };
            let d3 = [n(0), n(1), n(2)];
            if d3 != [0, 0, 0] {
                split.push((*c, d.clone(), d3));
            }
        }
    }
    split
}

/// **A precomposed letter draws the same pixels on every arm.** A face that
/// lacks one draws it decomposed, a base and its marks (`baud_font`'s
/// normalisation, as HarfBuzz's), so one code point can shape to several
/// glyphs. Run over stock JetBrains Mono with the scene's faces behind it,
/// where U+01FA shapes to several and U+00E9 to one -- the premise -- and
/// over the macOS default chain: the embedded faces with the system's
/// behind them, in the app's order.
///
/// **And as its canonical decomposition typed** (`forms_gate`): the two are
/// one text (`[[source: unicode18-ch3]]` D70, C6), so each arm draws the
/// same pixels for both. Before, 1838 letter draws differed: on stock
/// JetBrains Mono a letter no face has drew its parts' boxes over the next
/// cell where its decomposition lost its marks, and a bold or italic letter
/// the styled face lacked fell to the regular face where its decomposition
/// stayed styled; on the default chain the letter came whole from Menlo,
/// Apple Symbols or STIX where its decomposition came apart in the
/// embedded face.
#[test]
fn every_precomposed_letter_draws_alike_on_every_arm() {
    let letters = precomposed_letters();
    for c in ['Ǻ', 'Ȁ', 'Ṩ', 'Ȅ', 'Ǡ', 'Ḉ', 'ǰ', 'ḉ', 'é'] {
        assert!(letters.contains(&c), "the premise: {c} is swept");
    }
    let read = |p: &str| std::fs::read(p).unwrap_or_else(|e| panic!("{p}: {e}"));
    let here = |p: &str| read(&format!("{}/../{p}", env!("CARGO_MANIFEST_DIR")));
    let theme = Theme {
        bg_alpha: baud_domain::Chan::from_u8(56),
        ..Theme::dark()
    };
    let fallback = |b: Vec<u8>, i: u32| (b, i, FaceRole::Fallback);
    let styled = |p: &str, s: FaceStyle| (here(p), 0, FaceRole::Style(s));
    let stock: Vec<(Vec<u8>, u32, FaceRole)> = vec![
        fallback(here("baud-font/tests/fonts/JetBrainsMono-Regular.ttf"), 0),
        fallback(here("baud-font/tests/fonts/NotoColorEmoji-subset.ttf"), 0),
        styled(
            "baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf",
            FaceStyle::Bold,
        ),
        styled(
            "baud-app/fonts/jetbrains-mono/JetBrainsMono-Italic.ttf",
            FaceStyle::Italic,
        ),
    ];
    // `default_faces` on macOS, in its order.
    let embedded: Vec<(Vec<u8>, u32, FaceRole)> = vec![
        fallback(here("baud-app/fonts/built/Regular.ttf"), 0),
        fallback(
            here("baud-app/fonts/symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf"),
            0,
        ),
        fallback(read("/System/Library/Fonts/Menlo.ttc"), 0),
        fallback(read("/System/Library/Fonts/Apple Color Emoji.ttc"), 0),
        fallback(read("/System/Library/Fonts/Apple Symbols.ttf"), 0),
        fallback(read("/System/Library/Fonts/Hiragino Sans GB.ttc"), 0),
        fallback(read("/System/Library/Fonts/AppleSDGothicNeo.ttc"), 0),
        fallback(read("/System/Library/Fonts/Kohinoor.ttc"), 0),
        fallback(
            here("baud-app/fonts/stix-two-math/STIXTwoMath-Regular.otf"),
            0,
        ),
        styled("baud-app/fonts/built/Bold.ttf", FaceStyle::Bold),
        styled("baud-app/fonts/built/Italic.ttf", FaceStyle::Italic),
        styled("baud-app/fonts/built/BoldItalic.ttf", FaceStyle::BoldItalic),
    ];
    let mut split = Vec::new();
    for (chain, files) in [("stock", &stock), ("embedded", &embedded)] {
        let faces: Vec<Face<'_>> = files
            .iter()
            .map(|(b, i, _)| Face::parse_index(b, baud_font::FaceIndex::of(*i)).expect("parse"))
            .collect();
        let roles: Vec<FaceRole> = files.iter().map(|(_, _, r)| *r).collect();
        let fresh = || {
            Renderer::new(
                Stack::with_roles(&faces, &roles).unwrap(),
                PxSize::of(14.0).unwrap(),
                theme.clone(),
                Blend::Native,
                Cols::of_index(usize::from(WS.ws_col)),
                Rows::of_index(usize::from(WS.ws_row)),
            )
            .unwrap()
        };
        if chain == "stock" {
            let mut renderer = fresh();
            let alone = |r: &mut Renderer<'_, '_>, c: char| {
                let cp = baud_domain::Codepoint::new(u32::from(c)).unwrap();
                r.cluster(&[cp], FaceStyle::Regular, baud_render::CellSpan::One)
                    .len()
            };
            assert!(
                alone(&mut renderer, 'Ǻ') > 1,
                "the premise: U+01FA shapes to several glyphs"
            );
            assert_eq!(
                alone(&mut renderer, 'é'),
                1,
                "the premise: U+00E9 shapes to one"
            );
        }
        for (name, sgr, style) in [
            ("regular", "", FaceStyle::Regular),
            ("bold", "\x1b[1m", FaceStyle::Bold),
            ("italic", "\x1b[3m", FaceStyle::Italic),
        ] {
            for (letter, d, [inst, cell, cpu]) in forms_gate(&fresh, &letters, sgr) {
                split.push(format!(
                    "{chain} {name} U+{:04X} {} against {:04X?}: the decomposition differs by {} px on the instance arm, {} on the cell arm, {} on the CPU arm",
                    u32::from(letter),
                    letter,
                    d.chars().map(u32::from).collect::<Vec<_>>(),
                    inst,
                    cell,
                    cpu
                ));
            }
            for (letter, gpu, soft, glyphs) in letters_gate(&fresh, &letters, sgr, style) {
                let cp = baud_domain::Codepoint::new(u32::from(letter)).unwrap();
                let mapped = faces.iter().any(|f| f.glyph(cp).is_some());
                split.push(format!(
                    "{chain} {name} U+{:04X} {}: cell arm {} px, CPU arm {} px, {} glyphs alone, {}",
                    u32::from(letter),
                    letter,
                    gpu,
                    soft,
                    glyphs,
                    if mapped { "mapped" } else { "unmapped" }
                ));
            }
        }
    }
    assert!(
        split.is_empty(),
        "{} letters draw differently across the arms: {split:#?}",
        split.len()
    );
}

/// The pointed Genesis 1:1-5 (`baud-core/tests/cluster_bounds.rs`), one
/// verse a row.
const GENESIS_1_1_TO_5: [&str; 5] = [
    "בְּרֵאשִׁית בָּרָא אֱלֹהִים אֵת הַשָּׁמַיִם וְאֵת הָאָרֶץ׃",
    "וְהָאָרֶץ הָיְתָה תֹהוּ וָבֹהוּ וְחֹשֶׁךְ עַל־פְּנֵי תְהוֹם וְרוּחַ אֱלֹהִים מְרַחֶפֶת עַל־פְּנֵי הַמָּיִם׃",
    "וַיֹּאמֶר אֱלֹהִים יְהִי אוֹר וַיְהִי־אוֹר׃",
    "וַיַּרְא אֱלֹהִים אֶת־הָאוֹר כִּי־טוֹב וַיַּבְדֵּל אֱלֹהִים בֵּין הָאוֹר וּבֵין הַחֹשֶׁךְ׃",
    "וַיִּקְרָא אֱלֹהִים לָאוֹר יוֹם וְלַחֹשֶׁךְ קָרָא לָיְלָה וַיְהִי־עֶרֶב וַיְהִי־בֹקֶר יוֹם אֶחָד׃",
];

/// **Italic and bold-italic Hebrew draw alike on the instance, cell and
/// CPU arms.** The pointed Genesis 1:1-5 in regular, `SGR 3` and
/// `SGR 1;3`, on the embedded font's four built styles behind the icon
/// face, as the app stacks them: each arm draws every frame pixel for
/// pixel as the others do. The controls: the italic and bold-italic frames
/// differ from the regular one and from each other, so the italic faces'
/// Hebrew is what the arms agree on -- while the italic faces carried no
/// Hebrew, all three frames were the upright Regular's.
#[test]
fn italic_hebrew_draws_alike_on_every_arm() {
    let here = |p: &str| {
        std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR")))
            .unwrap_or_else(|e| panic!("{p}: {e}"))
    };
    let files = [
        here("baud-app/fonts/built/Regular.ttf"),
        here("baud-app/fonts/symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf"),
        here("baud-app/fonts/built/Bold.ttf"),
        here("baud-app/fonts/built/Italic.ttf"),
        here("baud-app/fonts/built/BoldItalic.ttf"),
    ];
    let faces: Vec<Face<'_>> = files
        .iter()
        .map(|d| Face::parse(d).expect("parse"))
        .collect();
    let roles = [
        FaceRole::Fallback,
        FaceRole::Fallback,
        FaceRole::Style(FaceStyle::Bold),
        FaceRole::Style(FaceStyle::Italic),
        FaceRole::Style(FaceStyle::BoldItalic),
    ];
    let theme = Theme {
        bg_alpha: Chan::from_u8(56),
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
    arms!(renderer => arms, t);
    // The cell arm reads the window copied in its drawn order, as the app
    // reads it: every verse is a right-to-left paragraph, and the core's
    // own grid holds its cells in the order they were written.
    arms.reads = Reads::Copy;
    let mut frames = Vec::new();
    for (name, sgr) in [
        ("regular", ""),
        ("italic", "\x1b[3m"),
        ("bold italic", "\x1b[1;3m"),
    ] {
        // Stored order: the glyphs and points are what this compares, so the
        // reordering a right-to-left row takes since BDSM starts set is off.
        let text = format!(
            "\x1bc\x1b[8l\x1b[?25l{sgr}{}",
            GENESIS_1_1_TO_5.join("\r\n")
        );
        feed(&mut t, text.as_bytes());
        assert_eq!(t.marks_dropped, 0, "{name}: the core keeps every point");
        let (a, b) = arms.draw(&mut t);
        let cpu = cpu_frame(&mut arms, &mut t);
        assert_eq!(
            channel_diff(&a, &b, |_| true),
            (0, 0),
            "{name}: the cell arm against the instance arm (pixels, largest channel step)"
        );
        assert_eq!(
            channel_diff(&a, &cpu, |_| true),
            (0, 0),
            "{name}: the CPU arm against the instance arm (pixels, largest channel step)"
        );
        frames.push(a);
    }
    assert!(frames[1] != frames[0], "italic Hebrew is the Regular's");
    assert!(
        frames[2] != frames[0],
        "bold-italic Hebrew is the Regular's"
    );
    assert!(frames[2] != frames[1], "bold-italic Hebrew is the Italic's");
}

/// **The config's notice on the cell path is the instance path's and the
/// CPU path's**: a row of its own after the screen's, blank in the window
/// copy and drawn over by the notice's field with baud's own overlays,
/// pixel for pixel against both, with and without a status line above it.
/// The arm that reads the core's grid in place is not compared: the app
/// draws from the copy, and the notice's row is no line of the core's.
#[test]
fn the_cell_path_draws_the_notice_as_the_instance_path_does() {
    let data = std::fs::read(format!(
        "{}/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("face");
    let faces = [Face::parse(&data).expect("parse")];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let bare = {
        arms!(fresh() => arms, t);
        scene(&mut t);
        arms.renderer.fit(&t);
        arms.draw(&mut t).0
    };
    let notice: Vec<baud_domain::Codepoint> =
        "~/.config/baud/config.toml line 12: \"font_size 15\" is not key = value"
            .chars()
            .map(baud_domain::Codepoint::from_char)
            .collect();
    let less = WinSize {
        ws_col: WS.ws_col,
        ws_row: WS.ws_row - 1,
    };
    // Each scene the first frame of a renderer of its own (`first_frame`),
    // its bytes fed to a terminal of its own.
    let opens = [
        (b"".as_slice(), "the notice"),
        (b"\x1b[1$~".as_slice(), "the notice under the indicator"),
    ];
    for k in 0..opens.len() {
        let name = opens[k].1;
        arms!(fresh() => arms, t);
        scene(&mut t);
        let _ = t.resize(&less);
        arms.renderer.set_notice(Some(notice.clone()));
        for (open, _) in &opens[..=k] {
            feed(&mut t, open);
        }
        arms.renderer.fit(&t);
        let (a, _) = arms.draw(&mut t);
        let c = arms.draw_copied(&mut t);
        let cpu = cpu_frame(&mut arms, &mut t);
        assert_eq!(a.len(), c.len(), "{name}");
        assert_eq!(differing(&a, &c), 0, "{name}: the cell arm disagrees");
        assert_eq!(differing(&a, &cpu), 0, "{name}: the CPU arm disagrees");
        assert_ne!(a, bare, "{name}: the notice reached the pixels");
        if k + 1 == opens.len() {
            feed(&mut t, b"\x1b[0$~");
            arms.renderer.set_notice(None);
            let _ = t.resize(&WS);
            arms.renderer.fit(&t);
            let (after, _) = arms.draw(&mut t);
            assert_eq!(after.len(), bare.len(), "the notice's row is gone");
        }
    }
}

/// **A reversed default page is raised alike on every arm**: the bar
/// `SGR 7` draws in the default colours and the `?5` screen, on the light
/// theme at its shipped dim, 153, over the glass, where each is drawn at
/// the lowest alpha its text reads at 4.5:1 over black to white, 173
/// (`Theme::reversed_alpha`; the reading itself is
/// `baud-render/tests/reverse_raise.rs`'s). Both GPU arms read it from the
/// one resolved table and the CPU arm, which the Linux driver draws with,
/// from the same slot, so one theme draws one set of pixels on every
/// system; and under `?5` the margin a presenter clears to is the page's
/// alpha, so the gutter carries no seam. The default page beside the bar
/// is the control: it keeps the dim.
#[test]
fn a_reversed_default_page_is_raised_alike_on_every_arm() {
    let data = std::fs::read(format!(
        "{}/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("face");
    let faces = [Face::parse(&data).expect("parse")];
    let theme = Theme {
        bg_alpha: Chan::from_u8(153),
        ..Theme::light()
    };
    let raised = theme.reversed_alpha(theme.fg, theme.bg).get();
    assert_eq!(raised, 173, "the premise: the light theme's raise");
    let renderer = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        theme,
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    arms!(renderer => arms, t);
    let m = arms.renderer.metrics();
    let side = |p: Px| usize::try_from(p.value()).unwrap();
    let (cw, ch) = (side(m.cell_w), side(m.cell_h));
    let fw = side(arms.renderer.frame_size().0);
    // The middle of cell `(row, col)`, a space, as the arms premultiply it.
    let page =
        |px: &[u32], row: usize, col: usize| px[(row * ch + ch / 2) * fw + col * cw + cw / 2];
    // A line of text under both, so the glyph map holds the scene's.
    let scenes: [(&str, &[u8], u32); 2] = [
        (
            "an SGR 7 bar",
            b"\x1b[7m status \x1b[0m\x1b[6;1HThe quick brown fox jumps over 0123456789\x1b[10;120H",
            153,
        ),
        (
            "the ?5 page",
            b"\x1bc\x1b[?5h page \x1b[6;1HThe quick brown fox jumps over 0123456789\x1b[10;120H",
            u32::from(raised),
        ),
    ];
    for (what, bytes, row_two) in scenes {
        feed(&mut t, bytes);
        let (a, b) = arms.draw(&mut t);
        assert_eq!(differing(&a, &b), 0, "{what}: the GPU arms disagree");
        let cpu = cpu_frame(&mut arms, &mut t);
        let d = channel_diff(&cpu, &a, |_| true);
        assert_eq!(d.0, 0, "{what}: the CPU arm differs by up to {}", d.1);
        assert_eq!(page(&a, 0, 0) >> 24, u32::from(raised), "{what}: raised");
        assert_eq!(page(&a, 2, 4) >> 24, row_two, "{what}: the page below");
        // Letterboxed: the margin around the frame is the default page.
        arms.margin = (8, 6);
        let (boxed, _) = arms.draw(&mut t);
        arms.margin = (0, 0);
        assert_eq!(boxed[0] >> 24, row_two, "{what}: the margin is the page");
    }
}

/// **The status line on the cell path is the instance path's**: its row
/// read from the window copy after the screen's and its glyphs drawn by
/// records, the indicator in reverse video and the host-writable line with
/// its cursor on it, pixel for pixel against the instance arm.
#[test]
fn the_cell_path_draws_the_status_line_as_the_instance_path_does() {
    let data = std::fs::read(format!(
        "{}/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("face");
    let faces = [Face::parse(&data).expect("parse")];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let bare = {
        arms!(fresh() => arms, t);
        scene(&mut t);
        feed(&mut t, b"\x1b[2 q");
        arms.renderer.fit(&t);
        arms.draw(&mut t).0
    };
    let opens = [
        (b"\x1b[1$~".as_slice(), "the indicator"),
        (
            b"\x1b[2$~\x1b[1$}mail: \x1b[1;31m3\x1b[0m".as_slice(),
            "the host-writable line, selected",
        ),
        (b"\x1b[0$}".as_slice(), "the host-writable line"),
        // Hebrew on a red ground, laid out from the right edge: its
        // backgrounds from the copy and its glyphs from the records agree
        // only if both are in the line's drawn order.
        (
            "\x1b[1$}\x1b[2K\x1b[41m\u{5E9}\u{5DC}\u{5D5}\u{5DD}\x1b[0m 42\x1b[0$}".as_bytes(),
            "a Hebrew host-writable line",
        ),
        (
            "\x1b[1$}\x1b[2K\x1b[41m\u{5E9}\u{5DC}\u{5D5}\u{5DD}\x1b[0m 42".as_bytes(),
            "a Hebrew host-writable line, selected",
        ),
    ];
    // Each scene the first frame of a renderer of its own (`first_frame`),
    // the bytes up to it fed to a terminal of its own.
    for k in 0..opens.len() {
        let name = opens[k].1;
        arms!(fresh() => arms, t);
        scene(&mut t);
        feed(&mut t, b"\x1b[2 q");
        for (open, _) in &opens[..=k] {
            feed(&mut t, open);
        }
        arms.renderer.fit(&t);
        let (a, _) = arms.draw(&mut t);
        let c = arms.draw_copied(&mut t);
        assert_eq!(a.len(), c.len(), "{name}");
        assert!(a.len() > bare.len(), "{name}: the frame grew a row");
        assert_eq!(differing(&a, &c), 0, "{name}: the arms disagree");
    }
}

/// Three targets that rotate as the presenter's surfaces do, and what each
/// holds, for the partial-redraw oracle.
struct Rotation {
    targets: Vec<baud_present_macos::Target>,
    ages: Ages,
    /// The surface the next frame goes to, and how many frames went.
    next: SurfaceIx,
    turns: Tally,
    /// Frames drawn by rows, and the rows they drew.
    partial: Tally,
    rows: Tally,
}

impl Rotation {
    fn new(gpu: &Gpu, (w, h): (Px, Px)) -> Rotation {
        Rotation {
            targets: (0..SURFACES)
                .map(|_| gpu.offscreen(w, h).expect("target"))
                .collect(),
            ages: Ages::new(),
            next: SurfaceIx::FIRST,
            turns: Tally::default(),
            partial: Tally::default(),
            rows: Tally::default(),
        }
    }

    /// The target `next` names: the turns go round the targets as the
    /// surfaces go round.
    fn target(&self) -> &baud_present_macos::Target {
        let n = u64::try_from(self.targets.len()).unwrap();
        &self.targets[usize::try_from(self.turns.value() % n).unwrap()]
    }

    /// A frame drawn: its change recorded against the target it went to,
    /// and counted when it is `counted`.
    fn drew(&mut self, f: &baud_present_macos::GpuFrame, counted: Counted) {
        self.ages.drew(self.next, f.changed);
        if let (baud_render::Redraw::Rows(set), Counted::Yes) = (f.drawn, counted) {
            self.partial.bump();
            self.rows.add(Tally::of(set.len().index()));
        }
        self.next = self.next.next();
        self.turns.bump();
    }
}

/// One step of the oracle: its name, the damage the app converts it with,
/// the change to the scene, and what its frame must have changed.
type Step = (&'static str, Damage, Act, Want);

/// A change to the scene, for [`cursor_steps`].
type Change = (&'static str, Damage, Act);

/// What a step does to the scene.
type Act = Box<dyn Fn(&mut Arms<'_, '_>, &mut Term<'_>)>;

/// The cursor moved to row `r`, column `c`, counted from one.
fn at(r: usize, c: usize) -> Act {
    Box::new(move |_, t| feed(t, format!("\x1b[{r};{c}H").as_bytes()))
}

/// The cursor's blink set to `p`.
fn blink(p: CursorPhase) -> Act {
    Box::new(move |a, _| a.renderer.set_cursor_blink(p))
}

/// What a step's frame must have changed of the one before: the positive
/// control that it was drawn by rows, so a pixel it got wrong would show.
#[derive(Clone, Copy, Debug)]
enum Want {
    /// Anything, the whole included.
    Any,
    /// Rows, any of them.
    Rows,
    /// Rows, these among them.
    Has(&'static [usize]),
    /// The whole.
    Whole,
}

impl Want {
    /// Whether `changed` is what this wants.
    fn met(self, changed: baud_render::Redraw) -> bool {
        match (self, changed) {
            (Want::Any, _) => true,
            (Want::Rows, baud_render::Redraw::Rows(_)) => true,
            (Want::Has(rows), baud_render::Redraw::Rows(set)) => {
                rows.iter().all(|&r| set.has(Row::from_index(r)))
            }
            (Want::Whole, baud_render::Redraw::Whole) => true,
            _ => false,
        }
    }
}

/// The oracle's steps: the cursor's own ([`cursor_steps`]), then the same
/// with the things that move beside the cursor on the screen
/// ([`ground_steps`]).
fn partial_steps() -> Vec<Step> {
    cursor_steps()
        .into_iter()
        .map(|(name, damage, change)| (name, damage, change, Want::Any))
        .chain(ground_steps())
        .collect()
}

/// The cursor's steps. The first is a content change; most of the rest
/// change only the cursor, its blink animation or its trail, so each of
/// those frames is drawn by rows. A text blink's flip is converted with
/// `Damage::Full`, as the app's tick hands it.
fn cursor_steps() -> Vec<Change> {
    let phase = |opacity: u8, glow: u8| CursorPhase {
        opacity: Chan::from_u8(opacity),
        glow: Chan::from_u8(glow),
        ..CursorPhase::HIDDEN
    };
    let ghost = |p: f32| {
        TrailLook::Ghost(Trail {
            from: (Row::from_index(1), Col::at_index(2)),
            to: (Row::from_index(6), Col::at_index(40)),
            progress: p,
            origin_dy: 0.0,
            lift: Px::of(0),
            strength: baud_render::Strength::Full,
        })
    };
    let mut steps: Vec<Change> = vec![(
        "the scene, the cursor a blinking block on row 2",
        Damage::Incremental,
        Box::new(|_, t| {
            scene(t);
            // Rows of powerline arrows above and below the cursor's row:
            // the Nerd Font's U+E0B0 reaches two pixels past its row at
            // either end at 14 px, so a band between them is drawn with
            // both of their rows.
            for row in [2, 4] {
                feed(t, format!("\x1b[{row};1H\x1b[2K").as_bytes());
                feed(t, "\u{e0b0} ".repeat(60).as_bytes());
            }
            feed(t, b"\x1b[1 q\x1b[3;5H");
        }),
    )];
    // Two onto an arrow, whose ink past its row changes colour under the
    // block: the rows it reaches change with the cursor's.
    for (r, c) in [(3, 9), (8, 30), (4, 1), (2, 3), (1, 1), (9, 119), (5, 60)] {
        steps.push((
            "the cursor moved",
            Damage::Incremental,
            Box::new(move |_, t| feed(t, format!("\x1b[{r};{c}H").as_bytes())),
        ));
    }
    // A row's cells rewritten away from the cursor, in glyphs and a
    // rendition the screen already shows, the cursor saved and put back:
    // nothing but the cells changes, so only the screen the frame names
    // says the row must be drawn.
    steps.push((
        "a row's cells rewritten, the cursor where it was",
        Damage::Incremental,
        Box::new(|_, t| feed(t, b"\x1b7\x1b[8;1Hworld\x1b8")),
    ));
    for p in [
        CursorPhase::HIDDEN,
        CursorPhase::SHOWN,
        CursorPhase::HIDDEN,
        CursorPhase::SHOWN,
    ] {
        steps.push((
            "the blink flipped",
            Damage::Incremental,
            Box::new(move |a, _| a.renderer.set_cursor_blink(p)),
        ));
    }
    for g in [128, 192, 64, 255] {
        steps.push((
            "a plasma phase",
            Damage::Incremental,
            Box::new(move |a, _| a.renderer.set_cursor_blink(phase(0, g))),
        ));
    }
    for at in [70, 80, 135, 205] {
        steps.push((
            "a fire step",
            Damage::Incremental,
            Box::new(move |a, _| {
                a.renderer
                    .set_cursor_blink(CursorPhase::fire(Chan::from_u8(at)))
            }),
        ));
    }
    for at in [10, 70, 128, 190] {
        steps.push((
            "a ufo step",
            Damage::Incremental,
            Box::new(move |a, _| {
                a.renderer
                    .set_cursor_blink(CursorPhase::ufo(Chan::from_u8(at)))
            }),
        ));
    }
    steps.push((
        "the blink shown again",
        Damage::Incremental,
        Box::new(|a, _| a.renderer.set_cursor_blink(CursorPhase::SHOWN)),
    ));
    for p in [0.1, 0.3, 0.6, 0.9] {
        steps.push((
            "a trail step",
            Damage::Incremental,
            Box::new(move |a, _| a.trail = Some(ghost(p))),
        ));
    }
    steps.push((
        "the trail gone",
        Damage::Incremental,
        Box::new(|a, _| a.trail = None),
    ));
    // A quad trail arriving at the cursor from row 7, and the fire trail's
    // particles twice: from the top row, where they rise past the grid, and
    // across the middle of the screen.
    for p in [0.05f32, 0.2, 0.4, 0.6] {
        steps.push((
            "a quad trail step",
            Damage::Incremental,
            Box::new(move |a, t| {
                let (cr, cc) = t.cursor();
                let hole = a.renderer.cursor_rect(cr, cc, t.cursor_style());
                let from = a
                    .renderer
                    .cursor_rect(Row::from_index(7), Col::at_index(60), t.cursor_style())
                    .corners();
                a.trail = Some(TrailLook::Smear(Smear {
                    corners: Segment::new(from, hole).at(p),
                    hole,
                    shape: Shape::Quad,
                    strength: baud_render::Strength::Full,
                    opacity: Smear::OPAQUE,
                }));
            }),
        ));
    }
    for ((c0, r0), (c1, r1), ages) in [
        (
            (98.0f32, 0.0f32),
            (116.0f32, 1.0f32),
            &[0.04f32, 0.12, 0.25, 0.4][..],
        ),
        (
            (60.0, 4.0),
            (100.0, 6.0),
            &[0.04, 0.12, 0.25, 0.4, 0.55, 0.7, 0.85],
        ),
    ] {
        for &age in ages {
            steps.push((
                "a fire trail step",
                Damage::Incremental,
                Box::new(move |a, _| {
                    let m = a.renderer.metrics();
                    let (cw, ch) = (m.cell_w.value() as f32, m.cell_h.value() as f32);
                    let cell = |c: f32, r: f32| Rect {
                        left: c * cw,
                        top: r * ch,
                        right: (c + 1.0) * cw,
                        bottom: (r + 1.0) * ch,
                    };
                    let (from, to) = (cell(c0, r0), cell(c1, r1));
                    let centre = |r: Rect| Pt {
                        x: (r.left + r.right) * 0.5,
                        y: (r.top + r.bottom) * 0.5,
                    };
                    let (fa, ta) = (centre(from), centre(to));
                    let l = (ta.x - fa.x).hypot(ta.y - fa.y);
                    let ignite = Ignition::ignite(fa, to, Seed(7), ch);
                    let life = (Effect::Fire.end(&ignite) / Effect::Fire.life()).min(1.0);
                    let age = age * life;
                    a.trail = Some(TrailLook::Vfx(Vfx {
                        effect: Effect::Fire,
                        emitter: Smear {
                            corners: Segment::new(from.corners(), to).at(age),
                            hole: to,
                            shape: Shape::Quad,
                            strength: baud_render::Strength::Full,
                            opacity: Smear::OPAQUE,
                        },
                        ignition: ignite.at(age),
                        axis: Pt {
                            x: (ta.x - fa.x) / l,
                            y: (ta.y - fa.y) / l,
                        },
                    }));
                }),
            ));
        }
    }
    steps.push((
        "the effect gone",
        Damage::Incremental,
        Box::new(|a, _| a.trail = None),
    ));
    steps.push((
        "text written, some of it blinking",
        Damage::Incremental,
        Box::new(|_, t| feed(t, b"\x1b[7;1Hmore \x1b[5mtext\x1b[25m\x1b[5;60H")),
    ));
    // Not the cursor's: the blinking text's phase, which the cell shader
    // hides by a uniform. Drawn whole, or the text keeps its old phase.
    for slow in [Blink::Hidden, Blink::Shown] {
        steps.push((
            "the text's blink flipped",
            Damage::Full,
            Box::new(move |a, _| {
                a.renderer.set_blink(BlinkPhases {
                    slow,
                    rapid: Blink::Shown,
                })
            }),
        ));
    }
    for (r, c) in [(5, 61), (5, 62), (6, 1), (2, 2)] {
        steps.push((
            "the cursor moved after text",
            Damage::Incremental,
            Box::new(move |_, t| feed(t, format!("\x1b[{r};{c}H").as_bytes())),
        ));
    }
    steps
}

/// **The steps where what moves beside the cursor is drawn**: iTerm2's
/// cursor guide on the cursor's row -- moved with a hidden cursor, which
/// changes no cursor word, and on the cursor's line while the window looks
/// back, where no cursor is drawn -- and under a hollow cursor; the badge,
/// with a block cursor moved into it and blinking there, which cuts the
/// badge round its cell; and the config's notice row below the grid, with
/// the cursor on the row above it among glyphs that reach into it. Each
/// step that changes only those names the rows its frame must change.
fn ground_steps() -> Vec<Step> {
    let look = Some(baud_render::GuideLook {
        colour: baud_render::CURSOR_GUIDE,
        line: Px::of(2),
    });
    let badge = |text: &str| baud_render::BadgeLook {
        text: text.into(),
        colour: baud_render::BADGE,
        outline: baud_render::Theme::dark()
            .bg
            .with_alpha(baud_render::DRAWN_AT),
        drawn_at: baud_render::DRAWN_AT,
        fraction: baud_render::BOX,
        top: Px::of(4),
        right: Px::of(10),
        scale: 1.0,
        style: FaceStyle::Bold,
    };
    let notice = |text: &str| -> Vec<baud_domain::Codepoint> {
        text.chars()
            .map(baud_domain::Codepoint::from_char)
            .collect()
    };
    let inc = Damage::Incremental;
    vec![
        (
            "the cursor guide on",
            inc,
            Box::new(move |a, _| a.guide = look),
            Want::Has(&[1]),
        ),
        (
            "the cursor moved under the guide",
            inc,
            at(6, 10),
            Want::Has(&[1, 5]),
        ),
        (
            "the blink hidden under the guide",
            inc,
            blink(CursorPhase::HIDDEN),
            Want::Has(&[5]),
        ),
        // No cursor word changes: only the guide's rows.
        (
            "the hidden cursor moved under the guide",
            inc,
            at(8, 3),
            Want::Has(&[5, 7]),
        ),
        (
            "the blink shown under the guide",
            inc,
            blink(CursorPhase::SHOWN),
            Want::Has(&[7]),
        ),
        (
            "lines scrolled into the history",
            inc,
            Box::new(|_, t| feed(t, b"\x1b[10;1H\n\n\n\n\x1b[8;3H")),
            Want::Any,
        ),
        (
            "looking back with the guide",
            inc,
            Box::new(|a, _| a.back = Rows::of_count(3)),
            Want::Any,
        ),
        // The live cursor's line, three rows down the window, takes the
        // guide; no cursor is drawn.
        (
            "the live cursor moved, looking back",
            inc,
            at(4, 6),
            Want::Has(&[6]),
        ),
        ("and again, looking back", inc, at(2, 6), Want::Has(&[4, 6])),
        (
            "a third time, looking back",
            inc,
            at(5, 8),
            Want::Has(&[4, 7]),
        ),
        (
            "a fourth time, looking back",
            inc,
            at(3, 2),
            Want::Has(&[5, 7]),
        ),
        (
            "the live screen again",
            inc,
            Box::new(|a, _| a.back = Rows::ZERO),
            Want::Any,
        ),
        (
            "the hollow cursor under the guide",
            inc,
            Box::new(|a, _| a.renderer.set_cursor_look(CursorLook::Hollow)),
            Want::Has(&[2]),
        ),
        ("the hollow cursor moved", inc, at(4, 7), Want::Has(&[2, 3])),
        (
            "the shaped cursor again",
            inc,
            Box::new(|a, _| a.renderer.set_cursor_look(CursorLook::Shaped)),
            Want::Has(&[3]),
        ),
        (
            "the cursor guide off",
            inc,
            Box::new(|a, _| a.guide = None),
            Want::Has(&[3]),
        ),
        (
            "the badge set",
            inc,
            Box::new(move |a, _| a.badge = Some(badge("prod-db-01\nmain"))),
            Want::Any,
        ),
        (
            "a block cursor moved into the badge",
            inc,
            Box::new(|a, t| {
                let rect = a.renderer.badge().iter().fold(
                    [i32::MAX, i32::MAX, i32::MIN, i32::MIN],
                    |r, i| {
                        let (x, y) = (i32::from(i.x.0), i32::from(i.y.0));
                        [
                            r[0].min(x),
                            r[1].min(y),
                            r[2].max(x + i32::from(i.w.0)),
                            r[3].max(y + i32::from(i.h.0)),
                        ]
                    },
                );
                assert!(rect[0] < rect[2], "the badge was drawn");
                let m = a.renderer.metrics();
                let (cw, ch) = (
                    i32::try_from(m.cell_w.value()).unwrap(),
                    i32::try_from(m.cell_h.value()).unwrap(),
                );
                let (row, col) = ((rect[1] + rect[3]) / 2 / ch, (rect[0] + rect[2]) / 2 / cw);
                feed(t, format!("\x1b[{};{}H", row + 1, col + 1).as_bytes());
            }),
            Want::Rows,
        ),
        (
            "the blink hidden in the badge",
            inc,
            blink(CursorPhase::HIDDEN),
            Want::Rows,
        ),
        (
            "the blink shown in the badge",
            inc,
            blink(CursorPhase::SHOWN),
            Want::Rows,
        ),
        (
            "the cursor moved out of the badge",
            inc,
            at(9, 5),
            Want::Rows,
        ),
        (
            "the badge changed",
            inc,
            Box::new(move |a, _| a.badge = Some(badge("staging\nmain"))),
            Want::Any,
        ),
        // Two frames that leave the badge as it is, so the target its
        // clearing goes to holds it.
        (
            "the cursor moved under the new badge",
            inc,
            at(7, 5),
            Want::Has(&[6, 8]),
        ),
        ("and moved again", inc, at(6, 5), Want::Has(&[5, 6])),
        (
            "the badge cleared",
            inc,
            Box::new(|a, _| a.badge = None),
            Want::Any,
        ),
        (
            "the notice shown, arrows on the row above it",
            inc,
            Box::new(move |a, t| {
                let less = WinSize {
                    ws_col: WS.ws_col,
                    ws_row: WS.ws_row - 1,
                };
                let _ = t.resize(&less);
                a.renderer.set_notice(Some(notice(
                    "~/.config/baud/config.toml line 12: \"font_size 15\" is not key = value",
                )));
                a.renderer.fit(t);
                feed(t, b"\x1b[9;1H\x1b[2K");
                feed(t, "\u{e0b0} ".repeat(60).as_bytes());
                feed(t, b"\x1b[5;5H");
            }),
            Want::Any,
        ),
        // The arrows reach into the notice's row: its band is drawn.
        (
            "the cursor moved onto the arrows above the notice",
            inc,
            at(9, 4),
            Want::Has(&[8, 9]),
        ),
        (
            "the blink hidden above the notice",
            inc,
            blink(CursorPhase::HIDDEN),
            Want::Has(&[9]),
        ),
        (
            "the blink shown above the notice",
            inc,
            blink(CursorPhase::SHOWN),
            Want::Has(&[9]),
        ),
        // Away from the notice, so the frames before its change hold no
        // row of it.
        (
            "the cursor moved away from the notice",
            inc,
            at(3, 3),
            Want::Has(&[8, 9]),
        ),
        ("the cursor moved again", inc, at(4, 4), Want::Has(&[2, 3])),
        ("and again", inc, at(5, 5), Want::Has(&[3, 4])),
        (
            "the notice changed",
            inc,
            Box::new(move |a, _| {
                a.renderer.set_notice(Some(notice(
                    "~/.config/baud/config.toml line 3: a list never closed",
                )))
            }),
            Want::Any,
        ),
        (
            "the cursor moved above the changed notice",
            inc,
            at(9, 30),
            Want::Has(&[9]),
        ),
        (
            "the notice gone",
            inc,
            Box::new(|a, t| {
                a.renderer.set_notice(None);
                let _ = t.resize(&WS);
                a.renderer.fit(t);
            }),
            Want::Any,
        ),
        (
            "the cursor moved after the notice",
            inc,
            at(3, 3),
            Want::Rows,
        ),
    ]
}

/// The faces the partial oracle draws with: JetBrains Mono, the Noto
/// subset and the Nerd Font symbols behind it.
fn partial_faces() -> [Vec<u8>; 3] {
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    [
        read("baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        read("baud-font/tests/fonts/NotoColorEmoji-subset.ttf"),
        read("baud-app/fonts/symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf"),
    ]
}

fn partial_renderer<'f, 'a>(faces: &'f [Face<'a>]) -> Renderer<'f, 'a> {
    let theme = Theme {
        bg_alpha: baud_domain::Chan::from_u8(56),
        ..Theme::dark()
    };
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        theme,
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap()
}

/// **A cursor-only frame on the cell path draws only its rows, and every
/// frame is the whole frame's pixels.** Three targets rotate as the
/// presenter's surfaces do, each frame drawn into the next by
/// `Gpu::draw_cells_since` with what that target lacks, then drawn whole
/// into a fourth and read back: the two must agree to the pixel, over the
/// glass, through cursor moves, blink flips, plasma and fire phases and
/// trail steps, and after a content change; and with the cursor guide, the
/// badge and the config's notice row ([`ground_steps`]).
#[test]
fn a_cursor_only_cell_frame_draws_its_rows_and_the_whole_frames_pixels() {
    let rot = cell_oracle(
        Reads::Copy,
        Px::of(0),
        partial_steps(),
        cursor_steps().len(),
    );
    rows_were_few(&rot);
    // And the cursor's own steps over the core's grid, read under its
    // rotation.
    let cursor = cursor_steps()
        .into_iter()
        .map(|(name, damage, change)| (name, damage, change, Want::Any))
        .collect::<Vec<Step>>();
    let n = cursor.len();
    rows_were_few(&cell_oracle(Reads::Core, Px::of(0), cursor, n));
}

/// **The same on the instance path**: each frame converted as the app
/// converts one, incrementally, and drawn by `Gpu::draw_since` into the
/// target the rotation gives it.
#[test]
fn a_cursor_only_instance_frame_draws_its_rows_and_the_whole_frames_pixels() {
    let rot = instance_oracle(Px::of(0), partial_steps(), cursor_steps().len());
    rows_were_few(&rot);
}

/// **A light that changes on its own line draws the frame whole, and a
/// cursor move beside it only its rows**, on both paths, the line below
/// the grid and above it, every frame against a whole draw: the line is
/// outside every row's band, so a frame that kept the target's pixels
/// there would show the light as it was. Perturbation: leave the lights'
/// records out of the frame's key -- the light's frame is drawn by rows,
/// over the line it left -- and this goes red.
#[test]
fn a_light_change_on_its_line_draws_the_frame_whole_on_both_paths() {
    use baud_render::{ByLed, Glow, Light, LineEdge, Shown};
    let lit = |rd: Light| -> Act {
        Box::new(move |a, _| {
            a.renderer
                .set_lights(ByLed { rd, ..Shown::DARK }, Glow::Full)
        })
    };
    let steps = || -> Vec<Step> {
        let inc = Damage::Incremental;
        let mut steps: Vec<Step> = cursor_steps()
            .into_iter()
            .take(1)
            .map(|(name, damage, change)| (name, damage, change, Want::Any))
            .collect();
        steps.extend::<Vec<Step>>(vec![
            ("the cursor moved", inc, at(3, 4), Want::Rows),
            ("RD lit", inc, lit(Light::Lit), Want::Whole),
            ("the cursor moved beside it", inc, at(5, 6), Want::Rows),
            ("RD dark", inc, lit(Light::Dark), Want::Whole),
            ("and the cursor again", inc, at(2, 9), Want::Rows),
        ]);
        steps
    };
    for edge in [LineEdge::Bottom, LineEdge::Top] {
        cell_oracle_lined((Reads::Copy, Px::of(0)), Some(edge), steps(), Counting(0));
        instance_oracle_lined(Px::of(0), Some(edge), steps(), Counting(0));
    }
}

/// **A guide across a legacy scroller's gutter is drawn whole**, on both
/// paths: the gutter is outside every band, so a frame that moves the
/// guide there draws every pixel, and still agrees with a whole draw.
#[test]
fn a_guide_across_the_gutter_is_drawn_whole_on_both_paths() {
    cell_oracle(Reads::Copy, Px::of(16), gutter_steps(), 0);
    instance_oracle(Px::of(16), gutter_steps(), 0);
}

/// The steps over a gutter: the scene, then the guide on and moved, with
/// the cursor shown and hidden, each frame whole; and the cursor moved
/// with the guide off, by rows.
fn gutter_steps() -> Vec<Step> {
    let look = Some(baud_render::GuideLook {
        colour: baud_render::CURSOR_GUIDE,
        line: Px::of(2),
    });
    let mut steps: Vec<Step> = cursor_steps()
        .into_iter()
        .take(3)
        .map(|(name, damage, change)| (name, damage, change, Want::Any))
        .collect();
    let inc = Damage::Incremental;
    steps.extend::<Vec<Step>>(vec![
        (
            "the cursor guide on",
            inc,
            Box::new(move |a, _| a.guide = look),
            Want::Whole,
        ),
        (
            "the cursor moved under the guide",
            inc,
            at(6, 10),
            Want::Whole,
        ),
        ("and again", inc, at(3, 4), Want::Whole),
        (
            "the blink hidden under the guide",
            inc,
            blink(CursorPhase::HIDDEN),
            Want::Whole,
        ),
        (
            "the hidden cursor moved under the guide",
            inc,
            at(8, 3),
            Want::Whole,
        ),
        ("and again", inc, at(2, 7), Want::Whole),
        (
            "the guide off, the blink shown",
            inc,
            Box::new(|a, _| {
                a.guide = None;
                a.renderer.set_cursor_blink(CursorPhase::SHOWN);
            }),
            Want::Whole,
        ),
        ("the cursor moved", inc, at(5, 5), Want::Rows),
        ("and again", inc, at(6, 6), Want::Rows),
    ]);
    steps
}

/// The partial oracle on the cell path over `steps`, the arm reading the
/// grid `reads` names, with a scroller's `gutter`: every frame against a
/// whole draw, and each against its [`Want`]. The rotation, its first
/// `counted` frames counted.
fn cell_oracle(reads: Reads, gutter: Px, steps: Vec<Step>, counted: usize) -> Rotation {
    cell_oracle_lined((reads, gutter), None, steps, Counting(counted))
}

/// How many of an oracle's first frames count toward the rows-drawn bound.
#[derive(Clone, Copy)]
struct Counting(usize);

/// [`cell_oracle`] with the activity lights' own line at `line`.
fn cell_oracle_lined(
    (reads, gutter): (Reads, Px),
    line: Option<baud_render::LineEdge>,
    steps: Vec<Step>,
    counted: Counting,
) -> Rotation {
    let counted = counted.0;
    let files = partial_faces();
    let faces = files.each_ref().map(|f| Face::parse(f).unwrap());
    let mut renderer = partial_renderer(&faces);
    renderer.set_gutter(gutter);
    renderer.set_lights_line(line, 1.0);
    arms!(renderer => arms, t);
    let size = arms.renderer.whole_size();
    let n = usize::try_from(size.0.value() * size.1.value()).unwrap();
    let whole = arms.gpu.offscreen(size.0, size.1).expect("target");
    let mut rot = Rotation::new(&arms.gpu, size);
    let mut wrong = Vec::new();
    // From the window's copy, as the app draws, the notice's row the
    // copy's, which the core's grid has none of; or from the core's grid.
    arms.reads = reads;
    for (i, (name, _, step, want)) in steps.iter().enumerate() {
        step(&mut arms, &mut t);
        arms.renderer.set_trail(arms.trail);
        let (partial, full, drawn, changed) = arms.with_cells(&mut t, |gpu, renderer, input| {
            let target = rot.target();
            let missing = rot.ages.missing(rot.next);
            let Some(Drawn::Frame(f)) =
                gpu.draw_cells_since(renderer, input, target, Done::Wait, missing)
            else {
                panic!("draw_cells_since");
            };
            let mut p = vec![0u32; n];
            gpu.read(target, &mut p).expect("read");
            let Some(Drawn::Frame(_)) = gpu.draw_cells(renderer, input, &whole, Done::Wait) else {
                panic!("draw_cells");
            };
            let mut q = vec![0u32; n];
            gpu.read(&whole, &mut q).expect("read");
            let count = match i < counted {
                true => Counted::Yes,
                false => Counted::No,
            };
            rot.drew(&f, count);
            (p, q, f.drawn, f.changed)
        });
        if differing(&partial, &full) != 0 {
            wrong.push(format!(
                "{i} {name}: {} pixels, drawn {drawn:?}",
                differing(&partial, &full)
            ));
        }
        if !want.met(changed) {
            wrong.push(format!("{i} {name}: changed {changed:?}, wanted {want:?}"));
        }
    }
    assert!(wrong.is_empty(), "cell path, gutter {gutter:?}: {wrong:#?}");
    rot
}

/// The partial oracle on the instance path, as [`cell_oracle`].
fn instance_oracle(gutter: Px, steps: Vec<Step>, counted: usize) -> Rotation {
    instance_oracle_lined(gutter, None, steps, Counting(counted))
}

/// [`instance_oracle`] with the activity lights' own line at `line`.
fn instance_oracle_lined(
    gutter: Px,
    line: Option<baud_render::LineEdge>,
    steps: Vec<Step>,
    counted: Counting,
) -> Rotation {
    let counted = counted.0;
    let files = partial_faces();
    let faces = files.each_ref().map(|f| Face::parse(f).unwrap());
    let mut renderer = partial_renderer(&faces);
    renderer.set_gutter(gutter);
    renderer.set_lights_line(line, 1.0);
    arms!(renderer => arms, t);
    let size = arms.renderer.whole_size();
    let n = usize::try_from(size.0.value() * size.1.value()).unwrap();
    let whole = arms.gpu.offscreen(size.0, size.1).expect("target");
    let mut rot = Rotation::new(&arms.gpu, size);
    let mut wrong = Vec::new();
    for (i, (name, damage, step, want)) in steps.iter().enumerate() {
        step(&mut arms, &mut t);
        arms.renderer.set_trail(arms.trail);
        arms.renderer.set_cursor_guide(arms.guide);
        arms.renderer.set_badge(arms.badge.clone());
        arms.renderer.set_images(&t, arms.back);
        arms.renderer.place_lights(&t);
        arms.renderer.convert(&t, arms.back, *damage);
        let target = rot.target();
        let missing = rot.ages.missing(rot.next);
        let Some(Drawn::Frame(f)) =
            arms.gpu
                .draw_since(&mut arms.renderer, target, Done::Wait, missing)
        else {
            panic!("draw_since");
        };
        let mut partial = vec![0u32; n];
        arms.gpu.read(target, &mut partial).expect("read");
        let Some(Drawn::Frame(_)) = arms.gpu.draw(&mut arms.renderer, &whole, Done::Wait) else {
            panic!("draw");
        };
        let mut full = vec![0u32; n];
        arms.gpu.read(&whole, &mut full).expect("read");
        let count = match i < counted {
            true => Counted::Yes,
            false => Counted::No,
        };
        rot.drew(&f, count);
        if differing(&partial, &full) != 0 {
            wrong.push(format!(
                "{i} {name}: {} pixels, drawn {:?}",
                differing(&partial, &full),
                f.drawn
            ));
        }
        if !want.met(f.changed) {
            wrong.push(format!(
                "{i} {name}: changed {:?}, wanted {want:?}",
                f.changed
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "instance path, gutter {gutter:?}: {wrong:#?}"
    );
    rot
}

/// The cursor's frames were drawn by rows, and each by few.
fn rows_were_few(rot: &Rotation) {
    assert!(
        rot.partial.value() >= 20,
        "the cursor-only frames were drawn by rows: {} of them",
        rot.partial.value()
    );
    assert!(
        rot.rows.value() < rot.partial.value() * u64::from(WS.ws_row) / 2,
        "and each by few: {} rows over {} frames",
        rot.rows.value(),
        rot.partial.value()
    );
}

/// **A right-to-left row draws in its visual order alike on every arm**,
/// and in the order a hand-reordered row draws in: while BDSM is set
/// (`[[source: ecma48]]` 7.2.1, IMPLICIT) each row is laid out in the order
/// UAX #9 gives it (`[[source: uax9-bidi]]`), on the instance arm, the cell
/// arm reading the window copy the app binds, and the CPU arm.
///
/// The expected frame is the same characters written already in their
/// visual order with BDSM reset, so it is drawn by the paths as they were
/// before any reordering existed: a row reordered on one arm and not
/// another, or reordered wrongly on all three, fails here. The scene
/// holds what the order moves -- a Hebrew word in a Latin line, brackets
/// L4 mirrors, a wide character inside a right-to-left run, pointed
/// letters the cell arm draws by records, and an underline across a run
/// -- and then a logical selection whose highlight the order splits, a
/// block cursor and a bar cursor over a right-to-left letter.
#[test]
fn right_to_left_rows_draw_in_their_visual_order_on_every_arm() {
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let built = read("baud-app/fonts/built/Regular.ttf");
    let noto = read("baud-font/tests/fonts/NotoColorEmoji-subset.ttf");
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&noto).expect("parse"),
    ];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    // Each row as written, and as UAX #9 draws it: the first row, whose
    // first strong letter is Latin, at paragraph level 0 with its run of R
    // reversed in place (L2); every row that starts with a Hebrew letter at
    // level 1 (P2-P3), reversed whole with its Latin run kept in order, and
    // drawn against the right edge with the two erased cells it was written
    // after, logically first, at the far right. A bracket pair resolved R
    // by N0 is drawn with its mirror (L4), the wide U+231A (ON, so R by N1
    // between two letters) kept whole with its lead on the left, a pointed
    // word's cells moved with their points, an underline drawn on the cells
    // it was set on, and a bracket mirrored though an RLM is stored on its
    // cell. The third field is a right-to-left row's width in cells.
    let (shin, lamed, vav, mem) = ("\u{5E9}", "\u{5DC}", "\u{5D5}", "\u{5DD}");
    let (alef, bet, gimel) = ("\u{5D0}", "\u{5D1}", "\u{5D2}");
    let (qamats, holam) = ("\u{5B8}", "\u{5B9}");
    let rows: [(String, String, Option<usize>); 7] = [
        (
            format!("abc {shin}{lamed}{vav}{mem} def"),
            format!("abc {mem}{vav}{lamed}{shin} def"),
            None,
        ),
        (
            format!("{alef}({bet}){gimel} xy"),
            format!("xy {gimel}({bet}){alef}"),
            Some(8),
        ),
        (
            format!("{alef}\u{231A}{bet} z"),
            format!("z {bet}\u{231A}{alef}"),
            Some(6),
        ),
        (
            format!("{shin}{qamats}{lamed}{vav}{holam}{mem}"),
            format!("{mem}{vav}{holam}{lamed}{shin}{qamats}"),
            Some(4),
        ),
        (
            format!("\x1b[4;31m{alef}{bet}\x1b[0m{gimel}"),
            format!("{gimel}\x1b[4;31m{bet}{alef}\x1b[0m"),
            Some(3),
        ),
        // A row with no right-to-left character, drawn where it is: and
        // the glyphs the harness requires of a scene.
        (
            "The quick brown fox jumps 0123456789".to_string(),
            "The quick brown fox jumps 0123456789".to_string(),
            None,
        ),
        // A bracket carrying an RLM, which the core stores on its cell:
        // still drawn with its mirror (L4), the RLM drawing nothing.
        (
            format!("{alef}({bet})\u{200F}{gimel} xy"),
            format!("xy {gimel}({bet}){alef}"),
            Some(8),
        ),
    ];
    let cols = usize::from(WS.ws_col);
    let scene = |t: &mut Term<'_>, implicit: bool, cursor: &str| {
        feed(t, b"\x1b[?25l\x1b[2J\x1b[H");
        feed(t, if implicit { b"\x1b[8h" } else { b"\x1b[8l" });
        for (i, (logical, visual, rtl)) in rows.iter().enumerate() {
            let (text, col) = match (implicit, rtl) {
                (true, _) => (logical, 3),
                (false, None) => (visual, 3),
                (false, Some(width)) => (visual, cols - 2 - width + 1),
            };
            feed(t, format!("\x1b[{};{col}H{text}", i + 2).as_bytes());
        }
        feed(t, cursor.as_bytes());
    };
    let three = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, what: &str| {
        arms.reads = Reads::Copy;
        let (a, _) = arms.draw(t);
        let b = arms.draw_copied(t);
        let c = cpu_frame(arms, t);
        assert_eq!(differing(&a, &b), 0, "{what}: the cell arm differs");
        assert_eq!(differing(&a, &c), 0, "{what}: the CPU arm differs");
        a
    };
    // The rows, no cursor and no selection: the frame of the visual order.
    {
        arms!(fresh() => arms, t);
        scene(&mut t, true, "");
        let got = three(&mut arms, &mut t, "reordered");
        assert!(arms.renderer.bidi().any(), "the rows were reordered");
        assert!(
            (0..usize::from(WS.ws_row)).all(|r| arms
                .renderer
                .bidi()
                .mirrors(Row::from_index(r))
                .is_empty()),
            "every bracket is drawn as its mirroring glyph"
        );
        scene(&mut t, false, "");
        let want = three(&mut arms, &mut t, "written in visual order");
        assert_eq!(differing(&got, &want), 0, "the visual order drawn");
        scene(&mut t, false, "");
        feed(&mut t, b"\x1b[8l");
        for (i, (logical, _, _)) in rows.iter().enumerate() {
            feed(&mut t, format!("\x1b[{};3H{logical}", i + 2).as_bytes());
        }
        let stored = three(&mut arms, &mut t, "explicit");
        assert!(
            differing(&stored, &want) > 0,
            "the control: BDSM reset draws the stored order"
        );
    }
    // **No shaping crosses a level run, nor runs inside a right-to-left
    // one** (UAX #9 3.5): `x->y` is one left-to-right run, which the face's
    // `->` ligature joins on every arm; the `->` between two Hebrew letters
    // resolves R, is drawn `<-` with the `>` mirrored, and each of its
    // cells is shaped alone -- by `convert_row`'s runs on two arms and by
    // the overlay's windows on the third, which must agree.
    {
        arms!(fresh() => arms, t);
        feed(&mut t, b"\x1b[?25l\x1b[2J\x1b[H\x1b[8h");
        feed(
            &mut t,
            format!("\x1b[2;3Hx->y {alef}->{bet} done").as_bytes(),
        );
        feed(
            &mut t,
            "\x1b[4;3HThe quick brown fox jumps 0123456789".as_bytes(),
        );
        let got = three(&mut arms, &mut t, "ligatures");
        feed(&mut t, b"\x1b[2J\x1b[8l");
        feed(
            &mut t,
            format!("\x1b[2;3Hx->y {alef}->{bet} done").as_bytes(),
        );
        feed(
            &mut t,
            "\x1b[4;3HThe quick brown fox jumps 0123456789".as_bytes(),
        );
        let stored = three(&mut arms, &mut t, "ligatures, explicit");
        assert!(differing(&got, &stored) > 0, "the row was reordered");
    }
    // A logical selection over `c ש` (the row is written from column 2):
    // drawn on the `c`, the space and the shin, which the order has put at
    // the far end of the word -- two runs of highlight, the same on every
    // arm.
    {
        arms!(fresh() => arms, t);
        scene(&mut t, true, "");
        arms.highlight = Some(Highlight::new(
            (Row::from_index(1), Col::at_index(4)),
            (Row::from_index(1), Col::at_index(6)),
            Region::Stream,
        ));
        let sel = three(&mut arms, &mut t, "selected");
        arms.highlight = None;
        let bare = three(&mut arms, &mut t, "unselected");
        let w = usize::try_from(arms.renderer.whole_size().0.value()).unwrap();
        let cw = usize::try_from(arms.renderer.metrics().cell_w.value()).unwrap();
        let ch = usize::try_from(arms.renderer.metrics().cell_h.value()).unwrap();
        let cell_differs = |col: usize| {
            (ch..2 * ch)
                .any(|y| (col * cw..(col + 1) * cw).any(|x| sel[y * w + x] != bare[y * w + x]))
        };
        // Logical 4..=6 is `c`, space, shin; the shin is drawn at visual
        // column 9, the end of `שלום` reversed, and the lamed at 8 and the
        // final mem at 6 are not selected.
        assert!(cell_differs(4) && cell_differs(5) && cell_differs(9));
        assert!(
            !cell_differs(8) && !cell_differs(6),
            "only the logical range"
        );
    }
    // **The pointed row, whose marked cells the cell arm draws by records**:
    // a selection of the shin and the lamed (logical 2 and 3, drawn at 117
    // and 116, the row a right-to-left paragraph against the right edge)
    // and a block cursor on the shin, each mapped through the order where
    // a record is painted as well as where the shader paints.
    {
        arms!(fresh() => arms, t);
        scene(&mut t, true, "");
        arms.highlight = Some(Highlight::new(
            (Row::from_index(4), Col::at_index(2)),
            (Row::from_index(4), Col::at_index(3)),
            Region::Stream,
        ));
        let sel = three(&mut arms, &mut t, "pointed, selected");
        arms.highlight = None;
        let bare = three(&mut arms, &mut t, "pointed, unselected");
        let w = usize::try_from(arms.renderer.whole_size().0.value()).unwrap();
        let cw = usize::try_from(arms.renderer.metrics().cell_w.value()).unwrap();
        let ch = usize::try_from(arms.renderer.metrics().cell_h.value()).unwrap();
        let cell_differs = |a: &[u32], b: &[u32], col: usize| {
            (4 * ch..5 * ch)
                .any(|y| (col * cw..(col + 1) * cw).any(|x| a[y * w + x] != b[y * w + x]))
        };
        let r4 = Row::from_index(4);
        let at = |c: usize| {
            arms.renderer
                .bidi()
                .visual_col(r4, Col::at_index(c))
                .index()
        };
        assert_eq!((at(2), at(3)), (cols - 3, cols - 4), "shin and lamed");
        assert!(cell_differs(&sel, &bare, cols - 4) && cell_differs(&sel, &bare, cols - 3));
        assert!(!cell_differs(&sel, &bare, 2) && !cell_differs(&sel, &bare, 3));
        assert!(
            !cell_differs(&sel, &bare, cols - 5) && !cell_differs(&sel, &bare, cols - 6),
            "only the logical range"
        );
        scene(&mut t, true, "\x1b[2 q\x1b[5;3H\x1b[?25h");
        let block = three(&mut arms, &mut t, "pointed, block cursor");
        assert!(
            cell_differs(&block, &bare, cols - 3),
            "the block over the shin, drawn at 117"
        );
        assert!(
            !cell_differs(&block, &bare, 2),
            "and not at its logical column"
        );
    }
    // A block and a bar cursor on the lamed (logical column 7): drawn over
    // visual column 8, the bar on the cell's right edge.
    for (style, name) in [("2", "block"), ("6", "bar")] {
        arms!(fresh() => arms, t);
        scene(&mut t, true, &format!("\x1b[{style} q\x1b[2;8H\x1b[?25h"));
        let got = three(&mut arms, &mut t, name);
        assert_eq!(
            arms.renderer
                .bidi()
                .visual_col(Row::from_index(1), Col::at_index(7)),
            Col::at_index(8)
        );
        scene(&mut t, true, "\x1b[2;8H\x1b[?25l");
        let hidden = three(&mut arms, &mut t, "no cursor");
        let w = usize::try_from(arms.renderer.whole_size().0.value()).unwrap();
        let cw = usize::try_from(arms.renderer.metrics().cell_w.value()).unwrap();
        let ch = usize::try_from(arms.renderer.metrics().cell_h.value()).unwrap();
        let changed: Vec<usize> = (0..w)
            .filter(|&x| (ch..2 * ch).any(|y| got[y * w + x] != hidden[y * w + x]))
            .collect();
        assert!(!changed.is_empty(), "{name}: drawn");
        assert!(
            changed.iter().all(|&x| x >= 8 * cw && x < 9 * cw),
            "{name}: inside visual column 8, {changed:?}"
        );
        if name == "bar" {
            assert!(
                changed.contains(&(9 * cw - 1)),
                "the bar is on the right edge"
            );
            assert!(!changed.contains(&(8 * cw)), "and not on the left");
        }
    }
}

/// **A finished paragraph the display re-breaks at its words draws the same
/// on every arm** (`[screen] wrap_words`, on by default): the English run
/// the terminal's wrap split moved to a line of its own, flush left, the
/// Hebrew around it against the right edge, the text above pushed up a
/// line, and a selection of stored cells on the moved line drawn where
/// they are drawn -- the cell shader's copy of the window, its instances
/// and the CPU compositor all reading the one display. With the setting
/// off the same bytes draw the terminal's cut, the control.
#[test]
fn a_re_broken_paragraph_draws_the_same_on_every_arm() {
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let built = read("baud-app/fonts/built/Regular.ttf");
    let noto = read("baud-font/tests/fonts/NotoColorEmoji-subset.ttf");
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&noto).expect("parse"),
    ];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let he =
        "\u{5D6}\u{5D5} \u{5E4}\u{5E1}\u{5E7}\u{5D4} \u{5D1}\u{5E2}\u{5D1}\u{5E8}\u{5D9}\u{5EA} ";
    let tail = "\u{5E9}\u{5DE}\u{5DE}\u{5E9}\u{5D9}\u{5DB}\u{5D4} \u{5DC}\u{5E9}\u{5D5}\u{5E8}\u{5D4} \u{5D4}\u{5D1}\u{5D0}\u{5D4}.";
    // Seven Hebrew phrases, 105 cells, then an English run across column
    // 120, then Hebrew; a prompt mark below it, so it is finished.
    let history: String = (0..12).map(|i| format!("h{i}\r\n")).collect();
    let bytes = format!(
        "\x1b[?25l{history}{}with English words in the middle, {tail}\r\n\x1b]133;A\x07$ ",
        he.repeat(7)
    );
    let three = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, what: &str| {
        arms.reads = Reads::Copy;
        let (a, _) = arms.draw(t);
        let b = arms.draw_copied(t);
        let c = cpu_frame(arms, t);
        assert_eq!(differing(&a, &b), 0, "{what}: the cell arm differs");
        assert_eq!(differing(&a, &c), 0, "{what}: the CPU arm differs");
        a
    };
    arms!(fresh() => arms, t);
    feed(&mut t, bytes.as_bytes());
    let on = three(&mut arms, &mut t, "re-broken");
    let d = arms
        .renderer
        .bidi()
        .display()
        .expect("the paragraph re-broken");
    assert_eq!(d.counts().0, Tally::of(1), "one paragraph");
    // The English line is display row 7, its "with" at the left edge,
    // stored on the paragraph's first row (window row 7) at column 105.
    assert_eq!(
        arms.renderer.bidi().pointer(Row::from_index(7), Col::ZERO),
        (Row::from_index(7), Col::at_index(105))
    );
    // A selection of the stored cells "with" (105..=108 of row 7): drawn
    // at visual 0..=3 of display row 7 on every arm, nowhere on row 6.
    arms.highlight = Some(Highlight::new(
        (Row::from_index(7), Col::at_index(105)),
        (Row::from_index(7), Col::at_index(108)),
        Region::Stream,
    ));
    let sel = three(&mut arms, &mut t, "re-broken, selected");
    arms.highlight = None;
    let w = usize::try_from(arms.renderer.whole_size().0.value()).unwrap();
    let cw = usize::try_from(arms.renderer.metrics().cell_w.value()).unwrap();
    let ch = usize::try_from(arms.renderer.metrics().cell_h.value()).unwrap();
    let cell_differs = |a: &[u32], b: &[u32], row: usize, col: usize| {
        (row * ch..(row + 1) * ch)
            .any(|y| (col * cw..(col + 1) * cw).any(|x| a[y * w + x] != b[y * w + x]))
    };
    assert!(
        (0..4).all(|c| cell_differs(&sel, &on, 7, c)),
        "the selection drawn"
    );
    assert!(!cell_differs(&sel, &on, 7, 4), "and only its cells");
    assert!(
        (100..120).all(|c| !cell_differs(&sel, &on, 6, c)),
        "not at the stored columns on the Hebrew line"
    );
    // The control: off draws the terminal's cut.
    arms.renderer.set_word_wrap(baud_render::WordWrap::Off);
    let off = three(&mut arms, &mut t, "the setting off");
    assert!(arms.renderer.bidi().display().is_none());
    assert!(differing(&on, &off) > 0, "the control: off draws otherwise");
}

/// **A selection of the cells a window line draws from above the window
/// is drawn the same on every arm**: "Y" ten times, cut after its sixth
/// letter at the window's top, is drawn whole on the window's first line,
/// its first six letters stored on the row above the window. Those six
/// selected, and none of the window's own rows, the cell arm's shader
/// still reads the overlay's answer for them, and every arm draws the six
/// cells selected and nothing else.
#[test]
fn a_selection_from_above_the_window_draws_the_same_on_every_arm() {
    use baud_render::{Beyond, Grain, Scrolled, Selection, Span};
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let built = read("baud-app/fonts/built/Regular.ttf");
    let noto = read("baud-font/tests/fonts/NotoColorEmoji-subset.ttf");
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&noto).expect("parse"),
    ];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let history: String = (0..12).map(|i| format!("h{i}\r\n")).collect();
    let fillers: String = (1..=8)
        .map(|i| format!("line {i}: the quick brown fox jumps over the lazy dog\r\n"))
        .collect();
    let bytes = format!(
        "\x1b[?25l{history}{} {} zz\r\n{fillers}\x1b]133;A\x07$ ",
        "X".repeat(113),
        "Y".repeat(10)
    );
    arms!(fresh() => arms, t);
    feed(&mut t, bytes.as_bytes());
    let scrolled = Scrolled::START.advanced(t.looking_back(Rows::of_index(usize::MAX)));
    let top = scrolled.top(Rows::ZERO);
    let up = top.above().expect("a line above");
    let up_row = t.view(Rows::of_count(1)).row(Row::ZERO).to_vec();
    let mut part = Selection::at(Span::cell(&up_row, up, Col::at_index(114)), Grain::Cell);
    part.extend(Span::cell(&up_row, up, Col::at_index(119)));
    let (cols, rows) = (usize::from(WS.ws_col), usize::from(WS.ws_row));
    let h = part
        .highlight_reaching(
            top,
            Rows::of_index(rows),
            Cols::of_index(cols),
            Beyond::of(&t, Rows::ZERO),
        )
        .expect("drawn on the first line");
    assert!(
        (0..rows).all(|r| !h.covers_row(Row::from_index(r))),
        "the premise: no window row's own cells"
    );
    let plain = three_arms(&mut arms, &mut t, "unselected");
    assert!(
        arms.renderer.bidi().display().is_some(),
        "the premise: the paragraph is drawn at its words"
    );
    arms.highlight = Some(h);
    let lit = three_arms(&mut arms, &mut t, "selected above the window");
    arms.highlight = None;
    let w = usize::try_from(arms.renderer.whole_size().0.value()).unwrap();
    let cw = usize::try_from(arms.renderer.metrics().cell_w.value()).unwrap();
    let ch = usize::try_from(arms.renderer.metrics().cell_h.value()).unwrap();
    assert_eq!(
        diff_box(&plain, &lit, w, (cw, ch)),
        Some((0, 0, 5, 0)),
        "the six cells of the first line, and only they"
    );
}

/// **The line a smooth scroll slides out is the one the display draws**:
/// a double-width line stored just above the window, over a finished
/// paragraph the display draws on one line more, is pushed out of the
/// window one line up, whose first line is the single-width "x1". The
/// leaving row's row of the mask is not marked sized: its size is read
/// from the display's line, not from the stored row, whose double width
/// marked it. A full-row slide is that window drawn still, on every arm.
#[test]
fn a_slide_under_a_re_broken_paragraph_draws_the_display_line() {
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let built = read("baud-app/fonts/built/Regular.ttf");
    let noto = read("baud-font/tests/fonts/NotoColorEmoji-subset.ttf");
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&noto).expect("parse"),
    ];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    // Seventeen thirteen-letter words: two rows at 120 columns, three lines
    // at their words (eight a line).
    let words: Vec<String> = (0..17)
        .map(|i| char::from(b'a' + i).to_string().repeat(13))
        .collect();
    let history: String = (0..12).map(|i| format!("h{i}\r\n")).collect();
    let fillers: String = (1..=7).map(|i| format!("x{i}\r\n")).collect();
    let bytes = format!(
        "\x1b[?25l{history}\x1b#6double width\r\n{fillers}{}\r\n\x1b]133;A\x07$ ",
        words.join(" ")
    );
    arms!(fresh() => arms, t);
    feed(&mut t, bytes.as_bytes());
    assert!(
        t.view(Rows::of_count(1)).rendition(Row::ZERO).is_double(),
        "the premise: the stored row above the window is double-width"
    );
    arms.reads = Reads::Copy;
    let cell_h = arms.renderer.metrics().cell_h;
    arms.back = Rows::ZERO;
    arms.slide = Some(cell_h);
    let (_, full_slid) = arms.draw(&mut t);
    // The leaving row's row of the mask, after the window's: no cell of it
    // marked sized.
    let (cols, rows) = (usize::from(WS.ws_col), usize::from(WS.ws_row));
    assert!(
        arms.overlay_mem[rows * cols..(rows + 1) * cols]
            .iter()
            .all(|&b| b & baud_render::MASK_SIZED == 0),
        "the leaving row is marked sized"
    );
    arms.slide = None;
    arms.back = Rows::of_count(1);
    let (instance_up, one_up) = arms.draw(&mut t);
    let cpu_up = cpu_frame(&mut arms, &mut t);
    assert!(
        arms.renderer.bidi().display().is_some(),
        "the premise: the window one line up is re-broken"
    );
    arms.back = Rows::ZERO;
    for (arm, up) in [
        ("cell", &one_up),
        ("instance", &instance_up),
        ("cpu", &cpu_up),
    ] {
        assert_eq!(
            differing(&full_slid, up),
            0,
            "a full-row slide under a re-broken paragraph is not the {arm} arm's screen one line up"
        );
    }
}

/// The faces the right-to-left gates draw with: baud's built face, which
/// holds the Hebrew letters, and the emoji subset.
fn rtl_faces() -> (Vec<u8>, Vec<u8>) {
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    (
        read("baud-app/fonts/built/Regular.ttf"),
        read("baud-font/tests/fonts/NotoColorEmoji-subset.ttf"),
    )
}

/// **A right-to-left line slides out in the order it was drawn in**, on
/// every frame of a smooth scroll: the leaving row is the line just above
/// the window, and BDSM set (`[[source: ecma48]]` 7.2.1, IMPLICIT, "All
/// bi-directional aspects of data are handled by the device itself") has
/// every display line drawn in the order UAX #9 L1-L2 give it
/// (`[[source: uax9-bidi]]`), the line the slide draws above the window
/// as much as the lines inside it. The line is a Hebrew paragraph, right
/// to left against the right edge, with a bracket pair L4 mirrors.
///
/// The expected frame is that line drawn still, as the first row of the
/// window one line up, where every arm agrees: a full-row slide is that
/// frame exactly, and a half-row slide is it moved down by the pan, pixel
/// for pixel wherever both draw. Before the leaving row went through the
/// layout it was drawn in its stored order, the Hebrew letters left to
/// right from the left edge. The control: with BDSM reset the same slide
/// draws another frame, so the order is what reached the pixels. A
/// double-width line (DECDWL) slides out the same way, drawn by the records
/// its conversion makes through the same order and its map: a Latin line
/// whose Hebrew run holds an arrow, drawn `<-` with each cell shaped alone
/// (UAX #9 3.5), and a U+2211, which has no mirroring glyph and is drawn
/// flipped (L4).
#[test]
fn a_right_to_left_line_slides_out_in_the_order_it_was_drawn() {
    let (built, noto) = rtl_faces();
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&noto).expect("parse"),
    ];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let rows = usize::from(WS.ws_row);
    // The Hebrew line, then a screen of Latin lines, so it is the one line
    // the screen has scrolled past: the leaving row of a slide at the live
    // screen.
    let hebrew = "\u{5D0}\u{5D1}(\u{5D2})\u{5D3} xy";
    let arrow = "ab \u{5D4}->\u{5D5}\u{2211}\u{5D6} xy";
    let scene = |t: &mut Term<'_>, mode: &str, width: &str, line: &str| {
        feed(t, b"\x1bc\x1b[?25l");
        feed(t, format!("{mode}{width}{line}").as_bytes());
        for i in 1..=rows {
            feed(
                t,
                format!("\r\nThe quick brown fox {i:02} jumps 0123456789").as_bytes(),
            );
        }
    };
    arms!(fresh() => arms, t);
    arms.reads = Reads::Copy;
    let ch = usize::try_from(arms.renderer.metrics().cell_h.value()).unwrap();
    let fw = usize::try_from(arms.renderer.frame_size().0.value()).unwrap();
    let row = ch * fw;
    let mut slides = Vec::new();
    for (mode, width, line) in [
        ("", "", hebrew),
        ("\x1b[8l", "", hebrew),
        ("", "\x1b#6", arrow),
    ] {
        scene(&mut t, mode, width, line);
        // Still: the line as the window's first row, one line up, alike on
        // every arm.
        arms.back = Rows::of_count(1);
        arms.slide = None;
        let (a, b) = arms.draw(&mut t);
        let c = arms.draw_copied(&mut t);
        let cpu = cpu_frame(&mut arms, &mut t);
        assert_eq!(
            arms.renderer.bidi().reordered(Row::ZERO),
            mode.is_empty(),
            "the premise: the line is reordered while BDSM is set"
        );
        for (arm, got) in [("cell", &b), ("copied", &c), ("cpu", &cpu)] {
            let d = differing(&got[..rows * row], &a[..rows * row]);
            assert_eq!(
                d, 0,
                "{mode:?}{width:?}: still, the {arm} arm differs, {d} pixels"
            );
        }
        arms.back = Rows::ZERO;
        for pan in [ch, ch / 2] {
            arms.slide = Some(Px::of(u32::try_from(pan).unwrap()));
            let (_, slid) = arms.draw(&mut t);
            let copied = arms.draw_copied(&mut t);
            let off = (ch - pan) * fw;
            let n = rows * row - off;
            for (arm, got) in [("cell", &slid), ("copied", &copied)] {
                let d = differing(&got[..n], &a[off..off + n]);
                assert_eq!(
                    d, 0,
                    "{mode:?}{width:?}: a slide of {pan} on the {arm} arm is not the line drawn \
                     still, {d} pixels"
                );
            }
            if pan == ch && width.is_empty() {
                slides.push(slid);
            }
        }
        arms.slide = None;
    }
    assert!(
        differing(&slides[0][..row], &slides[1][..row]) > 0,
        "the control: with BDSM reset the leaving row draws its stored order"
    );
}

/// **A line sliding out is shaped as it is drawn still**, on every frame
/// of a smooth scroll, whether BDSM is set or reset: the leaving row is
/// the line just above the window, and the cell path draws what its map
/// does not -- a letter in its joining form (`[[source: unicode-ch9]]`
/// 9.2.2, "an appropriate glyph to depict each Arabic letter according to
/// its immediate joining context"), a lam-alef ligature, a mirrored
/// character with no mirroring glyph flipped (UAX #9 L4), a mark on its
/// letter and a ligature -- by the records it draws them by on a row of
/// the window, walked over that row through the order it was drawn in.
/// Drawn from the map alone, the leaving row showed each Arabic letter
/// unjoined, U+2211 unflipped and the qamats missing until the slide
/// ended.
///
/// The expected frame is the line drawn still, as the first row of the
/// window one line up, where every arm agrees; a full-row slide is that
/// frame exactly and a half-row slide it moved down by the pan. The line
/// is underlined in part, so a record drawn after the decoration pass
/// would cover the line under it. A double-width right-to-left line,
/// drawn by records, slides out the same way, against its half's right
/// edge, and so do italics SAPV 17 slants left, with BDSM set and reset.
#[test]
fn a_line_sliding_out_is_shaped_as_it_is_drawn_still() {
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let built = read("baud-app/fonts/built/Regular.ttf");
    let italic = read("baud-app/fonts/built/Italic.ttf");
    let courier = std::fs::read("/System/Library/Fonts/Supplemental/Courier New.ttf")
        .expect("Courier New, which macOS ships; this test cannot skip itself");
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&courier).expect("parse"),
        Face::parse(&italic).expect("parse"),
    ];
    let roles = [
        FaceRole::Fallback,
        FaceRole::Fallback,
        FaceRole::Style(FaceStyle::Italic),
    ];
    let fresh = || {
        Renderer::new(
            Stack::with_roles(&faces, &roles).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let rows = usize::from(WS.ws_row);
    // An Arabic paragraph: "bayt" with a fatha on its beh, a lam-alef
    // pair, U+2211, which has no mirroring glyph, a bracket pair, a Latin
    // run holding `->`, and a pointed shin; "bayt" and the Latin run are
    // underlined in red, which a glyph drawn after the line would cover.
    let arabic = "\x1b[4;58:5:1m\u{628}\u{64E}\u{64A}\u{62A}\x1b[24m \u{644}\u{627} \u{2211}(\u{628}) \x1b[4ma->b\x1b[24;59m \u{5E9}\u{5B8}\u{5DC} xy";
    let scene = |t: &mut Term<'_>, mode: &str, width: &str, line: &str| {
        feed(t, b"\x1bc\x1b[?25l");
        feed(t, format!("{mode}{width}{line}").as_bytes());
        for i in 1..=rows {
            feed(
                t,
                format!("\r\nThe quick brown fox {i:02} jumps 0123456789").as_bytes(),
            );
        }
    };
    arms!(fresh() => arms, t);
    arms.reads = Reads::Copy;
    let ch = usize::try_from(arms.renderer.metrics().cell_h.value()).unwrap();
    let fw = usize::try_from(arms.renderer.frame_size().0.value()).unwrap();
    let row = ch * fw;
    for (mode, width, line) in [
        ("", "", arabic),
        ("\x1b[8l", "", arabic),
        ("\x1b[8l\x1b[2 k", "", arabic),
        ("", "\x1b#6", "\u{5D0}(\u{5D1})\u{5D2} \u{2211}"),
        (
            "",
            "",
            "\x1b[3m\x1b[17 ]\u{5E9}\u{5DC}\u{5D5}\u{5DD} (\u{2211}) ok\x1b[0m\x1b[0 ]",
        ),
        (
            "\x1b[8l\x1b[2 k",
            "",
            "\x1b[3m\x1b[17 ]\u{5E9}\u{5DC}\u{5D5}\u{5DD} (\u{2211}) ok\x1b[0m\x1b[0 ]",
        ),
    ] {
        scene(&mut t, mode, width, line);
        arms.back = Rows::of_count(1);
        arms.slide = None;
        let (a, b) = arms.draw(&mut t);
        let c = arms.draw_copied(&mut t);
        let cpu = cpu_frame(&mut arms, &mut t);
        assert!(
            arms.renderer.bidi().reordered(Row::ZERO)
                || arms.renderer.bidi().joins(Row::ZERO).is_some(),
            "{mode:?}{width:?}: the premise, the line is reordered or joined"
        );
        assert!(
            !line.contains("[17 ]") || !arms.renderer.bidi().slants(Row::ZERO).is_empty(),
            "{mode:?}: the premise, 17 slants the line's italics"
        );
        for (arm, got) in [("cell", &b), ("copied", &c), ("cpu", &cpu)] {
            let d = differing(&got[..rows * row], &a[..rows * row]);
            assert_eq!(
                d, 0,
                "{mode:?}{width:?}: still, the {arm} arm differs, {d} pixels"
            );
        }
        arms.back = Rows::ZERO;
        for pan in [ch, ch / 2] {
            arms.slide = Some(Px::of(u32::try_from(pan).unwrap()));
            let (_, slid) = arms.draw(&mut t);
            let copied = arms.draw_copied(&mut t);
            let off = (ch - pan) * fw;
            let n = rows * row - off;
            for (arm, got) in [("cell", &slid), ("copied", &copied)] {
                let d = differing(&got[..n], &a[off..off + n]);
                assert_eq!(
                    d, 0,
                    "{mode:?}{width:?}: a slide of {pan} on the {arm} arm is not the line drawn \
                     still, {d} pixels"
                );
            }
        }
        arms.slide = None;
    }
}

/// **A sliding line's joined letter reads its word, as drawn still**, on
/// every frame of a smooth scroll: the leaving row is laid out on its own
/// ([`Renderer::copy_leaving`]), and its joined cells are shaped with the
/// words that layout gives them, not the window's first row's. On Noto
/// Sans Syriac an alaph's `rlig` picks a wider final after a beth than
/// after a yudh (`a_joined_letter_reads_its_word_on_every_arm`); the
/// sliding line's alaph follows a beth and the window's first row holds
/// the same columns with a yudh, with BDSM set and reset; and a beth
/// ending the sliding line has its alaph on the window's first row, a
/// soft wrap between them. The expected frame is the line drawn still as
/// the window's first row, one line up, where every arm agrees.
#[test]
fn a_sliding_lines_letter_reads_its_word_as_drawn_still() {
    let built = std::fs::read(format!(
        "{}/../baud-app/fonts/built/Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("face");
    let syriac = std::fs::read("/System/Library/Fonts/Supplemental/NotoSansSyriac-Regular.ttf")
        .expect("Noto Sans Syriac, which macOS ships; this test cannot skip itself");
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&syriac).expect("parse"),
    ];
    let renderer = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    arms!(renderer => arms, t);
    arms.reads = Reads::Copy;
    let rows = usize::from(WS.ws_row);
    let cols = usize::from(WS.ws_col);
    let ch = usize::try_from(arms.renderer.metrics().cell_h.value()).unwrap();
    let fw = usize::try_from(arms.renderer.frame_size().0.value()).unwrap();
    let row = ch * fw;
    let (beth, yudh, alaph) = ('\u{0712}', '\u{071D}', '\u{0710}');
    // The lines above the screen's, and the line the slide moves out of.
    let same = format!("\x1b[3G{beth}\x1b[C{alaph}\r\n\x1b[3G{yudh}\x1b[C{alaph}");
    // With BDSM reset on a right-to-left path an erased cell parts two
    // letters.
    let touching = format!("\x1b[4G{beth}{alaph}\r\n\x1b[4G{yudh}{alaph}");
    let wrapped = format!("\x1b[{cols}G{beth}{alaph}");
    for (mode, line) in [("", &same), ("\x1b[8l\x1b[2 k", &touching), ("", &wrapped)] {
        feed(&mut t, b"\x1bc\x1b[?25l");
        feed(&mut t, format!("{mode}{line}").as_bytes());
        for i in 1..rows {
            feed(
                &mut t,
                format!("\r\nThe quick brown fox {i:02} jumps 0123456789").as_bytes(),
            );
        }
        // The sliding line is the one above the window's first row.
        arms.back = Rows::of_count(1);
        arms.slide = None;
        let (a, b) = arms.draw(&mut t);
        let c = arms.draw_copied(&mut t);
        let cpu = cpu_frame(&mut arms, &mut t);
        assert!(
            arms.renderer.bidi().joins(Row::ZERO).is_some(),
            "{mode:?}{line:?}: the premise, the sliding line joins"
        );
        for (arm, got) in [("cell", &b), ("copied", &c), ("cpu", &cpu)] {
            let d = differing(&got[..rows * row], &a[..rows * row]);
            assert_eq!(
                d, 0,
                "{mode:?}{line:?}: still, the {arm} arm differs, {d} pixels"
            );
        }
        if line != &wrapped {
            // The premise: the two alaphs, after a beth and after a yudh,
            // draw apart.
            assert_ne!(
                crop_cell(&arms, &a, (0, 4)),
                crop_cell(&arms, &a, (1, 4)),
                "{mode:?}: the premise, the rlig reads the letter before it"
            );
        }
        arms.back = Rows::ZERO;
        for pan in [ch, ch / 2] {
            arms.slide = Some(Px::of(u32::try_from(pan).unwrap()));
            let (_, slid) = arms.draw(&mut t);
            let copied = arms.draw_copied(&mut t);
            let off = (ch - pan) * fw;
            let n = rows * row - off;
            for (arm, got) in [("cell", &slid), ("copied", &copied)] {
                let d = differing(&got[..n], &a[off..off + n]);
                assert_eq!(
                    d, 0,
                    "{mode:?}{line:?}: a slide of {pan} on the {arm} arm is not the line drawn \
                     still, {d} pixels"
                );
            }
        }
        arms.slide = None;
    }
}

/// **A double-width right-to-left line is drawn within its half, right
/// to left**, on every arm: UAX #9 3.4 reorders each display line, and a
/// double-width line's is its half (`[[source: dec-std-070]]` § 5.14.1;
/// `[[source: vt510-decdwl]]`), so a Hebrew paragraph sits against the
/// half's right edge, its brackets mirrored (L4). Laid out across the
/// screen's width its text landed in the half that is not drawn, and
/// nothing of it showed. The expected frame is the same line written in
/// its drawn order with BDSM reset, its last cell the half's last; with
/// BDSM reset and SCP's right-to-left path the stored line is reversed
/// within the half and draws it too. On a DECDWL line and on the top half
/// of a DECDHL one.
#[test]
fn a_double_width_right_to_left_line_is_drawn_within_its_half() {
    let (built, noto) = rtl_faces();
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&noto).expect("parse"),
    ];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let three = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, what: &str| {
        arms.reads = Reads::Copy;
        let (a, b) = arms.draw(t);
        let c = arms.draw_copied(t);
        let d = cpu_frame(arms, t);
        assert_eq!(differing(&a, &b), 0, "{what}: the cell arm differs");
        assert_eq!(differing(&a, &c), 0, "{what}: the copied arm differs");
        assert_eq!(differing(&a, &d), 0, "{what}: the CPU arm differs");
        a
    };
    let half = usize::from(WS.ws_col) / 2;
    let stored = "\u{5D0}\u{5D1} (\u{5D2})";
    let shown = "(\u{5D2}) \u{5D1}\u{5D0}";
    // DECDWL, and DECDHL's top half.
    for size in ["\x1b#6", "\x1b#3"] {
        let scene = |t: &mut Term<'_>, setup: &str, at: usize, line: &str| {
            feed(
                t,
                format!(
                    "\x1bc\x1b[?25l\x1b[2;1H{setup}{size}\x1b[2;{at}H{line}\x1b[4;3HThe quick brown fox"
                )
                .as_bytes(),
            );
        };
        arms!(fresh() => arms, t);
        scene(&mut t, "", 1, stored);
        let implicit = three(&mut arms, &mut t, "implicit");
        assert!(
            arms.renderer.bidi().reordered(Row::from_index(1)),
            "the premise: the line is reordered"
        );
        scene(&mut t, "\x1b[8l", half + 1 - shown.chars().count(), shown);
        let want = three(&mut arms, &mut t, "written in its drawn order");
        feed(&mut t, b"\x1b[2;1H\x1b[2K\x1b[4;3HThe quick brown fox");
        let blank = three(&mut arms, &mut t, "the line erased");
        assert!(differing(&want, &blank) > 0, "the expected line is drawn");
        let d = differing(&implicit, &want);
        assert_eq!(d, 0, "implicit: within the half, {d} pixels");
        scene(&mut t, "\x1b[8l\x1b[2 k", 1, stored);
        let reversed = three(&mut arms, &mut t, "explicit, right to left");
        let d = differing(&reversed, &want);
        assert_eq!(d, 0, "explicit, right to left: within the half, {d} pixels");
    }
}

/// **An input method's composition on a right-to-left row draws as the
/// same text written there**, on every arm: the row is laid out with the
/// composition's text in its cells, so its cells are drawn where UAX #9
/// puts them and the row's other cells move as a write would move them --
/// `cd` composed after `ab` in a Hebrew paragraph is drawn `abcd`, the
/// `ab` moved left to make room, where drawing it from the cursor's cell
/// rightward covered the `a` -- and the cells it covers are hidden where
/// the order draws them: composed over the `xy` of `abxy`, no `x` shows.
/// The reference is `composition.rs`'s: the same text written underlined
/// at the cursor, the cursor on the cell after it in logical order, which
/// the paragraph draws left of `abcd`. On a double-width line, laid out
/// over its half, the same: drawn at its stored columns, the composition
/// sat where the text it would be written as is not. Each composed as `cd`
/// and as `c(`, whose `(` resolves right to left there and is drawn `)`:
/// on a double-width line the cell arm hid the cells at the composition's
/// stored columns, not where the order draws them, so it drew that `)` a
/// second time, and composed over `xy` it drew those letters under the
/// composition.
#[test]
fn a_composition_on_a_right_to_left_row_draws_as_the_text_written() {
    let (built, noto) = rtl_faces();
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&noto).expect("parse"),
    ];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let three = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, what: &str| {
        arms.reads = Reads::Copy;
        let (a, live) = arms.draw(t);
        let b = arms.draw_copied(t);
        let c = cpu_frame(arms, t);
        assert_eq!(differing(&a, &live), 0, "{what}: the live cell arm differs");
        assert_eq!(differing(&a, &b), 0, "{what}: the copied cell arm differs");
        assert_eq!(differing(&a, &c), 0, "{what}: the CPU arm differs");
        a
    };
    // After `ab`, on erased cells, where the write moves `ab`; and over
    // `xy`, whose cells it covers where the order draws them.
    // `xy` is red, so the composition over it is drawn red, and written
    // red: the rendition is the logical cell's, wherever it is drawn.
    // On a double-width line too (DECDWL), which is laid out over its
    // half, so the composition is drawn at the cursor's drawn column there.
    for (line, red, width, text) in [
        ("\u{5E9}\u{5DC}\u{5D5}\u{5DD} ab", "", ""),
        (
            "\u{5E9}\u{5DC}\u{5D5}\u{5DD} ab\x1b[31mxy\x1b[39m",
            "\x1b[31m",
            "",
        ),
        ("\u{5E9}\u{5DC}\u{5D5}\u{5DD} ab", "", "\x1b#6"),
        (
            "\u{5E9}\u{5DC}\u{5D5}\u{5DD} ab\x1b[31mxy\x1b[39m",
            "\x1b[31m",
            "\x1b#6",
        ),
    ]
    .into_iter()
    .flat_map(|(l, r, w)| ["cd", "c("].map(move |text| (l, r, w, text)))
    {
        arms!(fresh() => arms, t);
        feed(
            &mut t,
            format!(
                "\x1bc\x1b[2 q\x1b[3;1H{width}{line}\x1b[4;3HThe quick brown fox jumps 0123456789\x1b[3;8H"
            )
            .as_bytes(),
        );
        let (row, col) = t.cursor();
        arms.composition = Some(Composition {
            row,
            col,
            text: text
                .chars()
                .map(baud_domain::Codepoint::from_char)
                .collect(),
            measure: baud_render::Measure::default(),
            hints: Vec::new(),
            caret: Caret::End,
        });
        let composed = three(
            &mut arms,
            &mut t,
            &format!("{line}{width:?} {text}: composed"),
        );
        assert!(
            arms.renderer.bidi().reordered(row),
            "the premise: the row is reordered"
        );
        arms.composition = None;
        let plain = three(&mut arms, &mut t, "plain");
        feed(
            &mut t,
            format!("{red}\x1b[4m{text}\x1b[24m\x1b[39m").as_bytes(),
        );
        let written = three(&mut arms, &mut t, "written");
        assert!(
            differing(&composed, &plain) > 0,
            "{line}{width:?}: the composition is drawn"
        );
        assert!(
            differing(&written, &plain) > 0,
            "{line}{width:?}: the written text is drawn"
        );
        let d = differing(&composed, &written);
        assert_eq!(
            d, 0,
            "{line}{width:?}: as the same text written underlined, {d} pixels"
        );
    }
}

/// **A composition the core clusters draws as the text written, on every
/// arm**: two CJK characters over four cells, a ZWJ family, a flag, an
/// emoji under `U+FE0F` and a spacing mark on its letter, composed over
/// a line of text -- and on a line drawn in reverse with `?2500` a box
/// line two cells wide and `∑`, mirrored, before and after a letter that
/// takes a joining form is drawn -- each equal on the cell,
/// copied-cell and CPU arms to the
/// same text written underlined at the cursor -- the cursor after it,
/// where the commit lands -- and each against its premise, the frame
/// without it. A vowel sign composed after its consonant and a skin tone
/// after its emoji, on a line drawn in order and in reverse, are drawn on
/// the cell before the cursor, equal to that cell rewritten underlined
/// with them ([`Composition::at_cursor`]). The consonant and vowel sign
/// are र and ु, a syllable the embedded font draws inside its one cell:
/// a wider one, क with ि, spills into the cell after it, and the
/// composition draws the spill over that cell where the same syllable
/// written draws it under, a defect of syllables wider than their cell
/// that predates the embedded Devanagari (the same with Kohinoor
/// Devanagari).
#[test]
fn a_clustered_composition_draws_as_its_text_written_on_every_arm() {
    let (built, noto) = rtl_faces();
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&noto).expect("parse"),
    ];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let three = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, what: &str| {
        arms.reads = Reads::Copy;
        let (a, _) = arms.draw(t);
        let b = arms.draw_copied(t);
        let c = cpu_frame(arms, t);
        assert_eq!(differing(&a, &b), 0, "{what}: the cell arm differs");
        assert_eq!(differing(&a, &c), 0, "{what}: the CPU arm differs");
        a
    };
    // Then on a line drawn in reverse with `?2500`, under "wide": a box
    // line two cells wide and a Bidi_Mirrored character with no mirror
    // glyph, each mirrored as its text written is.
    let plain =
        "\x1bc\x1b[2 q\x1b[3;1HThe quick brown fox jumps over the lazy dog 0123456789\x1b[3;5H";
    let mirrored = "\x1bc\x1b[2 q\x1b[8l\x1b[3;1H\x1b[?2500h\x1b[2 kThe quick brown fox jumps over the lazy dog 0123456789\x1b[3;5H";
    // `pre` written after "The quick ", the cursor after it.
    let headed = |pre: &str, at: usize, path: &str| {
        format!(
            "\x1bc\x1b[2 q\x1b[8l\x1b[3;1H{path}The quick {pre}brown fox jumps over the lazy dog\x1b[3;{at}H"
        )
    };
    let (ka, sign) = ("\u{930}", "\u{941}");
    let (thumb, tone) = ("\u{1f44d}", "\u{1f3fd}");
    use baud_render::Joins::{Seen, Unseen};
    for (scene, text, joins, redo) in [
        (plain.to_string(), "\u{4e2d}\u{6587}", Unseen, ("", 0)),
        (
            plain.to_string(),
            "\u{1f468}\u{200d}\u{1f469}",
            Unseen,
            ("", 0),
        ),
        (plain.to_string(), "\u{1f1ef}\u{1f1f5}", Unseen, ("", 0)),
        (plain.to_string(), "\u{2764}\u{fe0f}", Unseen, ("", 0)),
        (plain.to_string(), "a\u{903}b", Unseen, ("", 0)),
        (
            mirrored.to_string(),
            "\u{250C}\u{2500}\u{2502}",
            Unseen,
            ("", 0),
        ),
        (mirrored.to_string(), "a\u{2211}b", Unseen, ("", 0)),
        (
            mirrored.to_string(),
            "\u{250C}\u{2500}\u{2502}",
            Seen,
            ("", 0),
        ),
        (mirrored.to_string(), "a\u{2211}b", Seen, ("", 0)),
        (headed(ka, 12, ""), sign, Unseen, (ka, 1)),
        (headed(thumb, 13, ""), tone, Unseen, (thumb, 2)),
        (headed(ka, 12, "\x1b[2 k"), sign, Unseen, (ka, 1)),
        (headed(thumb, 13, "\x1b[2 k"), tone, Unseen, (thumb, 2)),
    ] {
        arms!(fresh() => arms, t);
        // After a letter that takes a joining form is drawn, the layout
        // with BDSM reset keeps the last frame's joins.
        if joins == Seen {
            let joined = "\x1b[8l\x1b[1;1HThe quick brown fox jumps over the lazy dog 0123456789 \u{628}\u{628}\u{628}";
            feed(&mut t, joined.as_bytes());
            three(&mut arms, &mut t, "joined");
        }
        assert_eq!(arms.renderer.joins_seen(), joins, "{text:?}: the layout");
        if scene == mirrored {
            t.set_ambiguous_width(baud_domain::AmbiguousWidth::Wide);
        }
        feed(&mut t, scene.as_bytes());
        let cps: Vec<_> = text
            .chars()
            .map(baud_domain::Codepoint::from_char)
            .collect();
        let c = Composition::at_cursor(&t, &cps, &[], Caret::End);
        let head = c.col.index() + redo.1 == t.cursor().1.index();
        arms.composition = Some(c);
        let composed = three(&mut arms, &mut t, "composed");
        arms.composition = None;
        let plain = three(&mut arms, &mut t, "plain");
        let (pre, back) = redo;
        // Back by cells, HPB (`[[source: ecma48]]` 8.3.58, the active data
        // position): `CUB` on a line drawn in reverse with BDSM reset moves
        // left on the screen, away from the head.
        let rewrite = match back {
            0 => String::new(),
            n => format!("\x1b[{n}j"),
        };
        feed(
            &mut t,
            format!("{rewrite}\x1b[4m{pre}{text}\x1b[24m").as_bytes(),
        );
        let written = three(&mut arms, &mut t, "written");
        assert!(
            differing(&composed, &plain) > 0,
            "{text:?}: the composition is drawn"
        );
        let d = differing(&composed, &written);
        assert_eq!(
            d, 0,
            "{text:?} {joins:?}: as the same text written, {d} pixels"
        );
        assert!(head, "{text:?}: its head");
    }
}

/// **A composition against a Devanagari word draws as its text written,
/// on every arm** (`baud-render/src/word.rs`): the letters written against
/// it that its own continue are one word with it once committed, which it
/// draws whole, the grid leaving their cells to it, unlit. On Kohinoor
/// Devanagari behind JetBrains Mono: कमल composed after a blank, the block
/// cursor after it; मल and म composed after a written क, once with the क
/// selected; क composed before a written ल, the block cursor after it on
/// ल lighting ल's syllable; म between a written क and ल;
/// क् before a written ष, the conjunct formed across them; मल after कि
/// under `?2027`; `x` composed against a written कमल; and under `?2027`
/// क्ति, which fits at no width, composed after a written क and a blank,
/// and written after a composed क, closing on its ink -- each equal on
/// the cell, copied-cell and CPU arms to the same text written underlined
/// at the cursor, the cursor after it, and each against its premise, the
/// frame without it. And with the input method's cursor on a syllable of
/// a composed word, the ink over its block alike on every arm.
#[test]
fn a_composition_against_a_devanagari_word_draws_as_written_on_every_arm() {
    let data = std::fs::read(format!(
        "{}/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("face");
    let deva = std::fs::read("/System/Library/Fonts/Kohinoor.ttc").expect("Kohinoor Devanagari");
    let faces = [
        Face::parse(&data).expect("parse"),
        Face::parse_index(&deva, baud_font::FaceIndex::of(0u32)).expect("parse"),
    ];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    // The क of `ab` and `क` on row 3, selected alone.
    let ka = Highlight::new(
        (Row::from_index(2), Col::at_index(2)),
        (Row::from_index(2), Col::at_index(2)),
        Region::Stream,
    );
    for (base, text, selected) in [
        ("ab ", "\u{915}\u{92e}\u{932}", None),
        ("ab\u{915}", "\u{92e}\u{932}", None),
        ("ab\u{915}", "\u{92e}", None),
        ("ab\u{915}", "\u{92e}\u{932}", Some(ka)),
        ("ab  \u{932}\x1b[2D", "\u{915}", None),
        ("ab \u{915} \u{932}\x1b[2D", "\u{92e}", None),
        ("ab  \u{937}\x1b[2D", "\u{915}\u{94d}", None),
        ("\x1b[?2027hab\u{915}\u{93f}", "\u{92e}\u{932}", None),
        ("ab  \u{915}\u{92e}\u{932}\x1b[4D", "x", None),
        ("\x1b[?2027h\u{915} ", "\u{915}\u{94d}\u{924}\u{93f}", None),
        (
            "\x1b[?2027h  \u{915}\u{94d}\u{924}\u{93f}\x1b[1G",
            "\u{915}",
            None,
        ),
    ] {
        arms!(fresh() => arms, t);
        arms.highlight = selected;
        feed(
            &mut t,
            format!(
                "\x1bc\x1b[2 q\x1b[5;1HThe quick brown fox jumps over the lazy dog 0123456789\x1b[3;1H{base}"
            )
            .as_bytes(),
        );
        let cps: Vec<_> = text
            .chars()
            .map(baud_domain::Codepoint::from_char)
            .collect();
        arms.composition = Some(Composition::at_cursor(&t, &cps, &[], Caret::End));
        let composed = three_arms(&mut arms, &mut t, &format!("{base:?} {text:?}: composed"));
        arms.composition = None;
        let plain = three_arms(&mut arms, &mut t, &format!("{base:?} {text:?}: plain"));
        feed(&mut t, format!("\x1b[4m{text}\x1b[24m").as_bytes());
        let written = three_arms(&mut arms, &mut t, &format!("{base:?} {text:?}: written"));
        assert!(
            differing(&composed, &plain) > 0,
            "{base:?} {text:?}: the composition is drawn"
        );
        let d = differing(&composed, &written);
        assert_eq!(d, 0, "{base:?} {text:?}: as written, {d} pixels");
    }
    // The input method's cursor on म of कमल composed: the block on its
    // cell, the word's ink over it in the cursor's text colour, alike on
    // every arm.
    arms!(fresh() => arms, t);
    feed(
        &mut t,
        b"\x1bc\x1b[2 q\x1b[5;1HThe quick brown fox jumps over the lazy dog 0123456789\x1b[3;1Hab ",
    );
    let cps: Vec<_> = "\u{915}\u{92e}\u{932}"
        .chars()
        .map(baud_domain::Codepoint::from_char)
        .collect();
    arms.composition = Some(Composition::at_cursor(&t, &cps, &[], Caret::At(1)));
    let _ = three_arms(&mut arms, &mut t, "the cursor on a composed syllable");
}

/// **A composition over text SAPV varies draws as its own text written,
/// on every arm** (`[[source: ecma48]]` 8.3.108): `xyz` composed over
/// digits value 2 draws as the Arabic script's is drawn as `xyz`, not as
/// the digits under it; and two vavs composed where value 17 slants one
/// italic cell of a Hebrew line, in that cell's rendition, both lean
/// left, as written they do, though the cell after the first is upright.
#[test]
fn a_composition_over_varied_text_draws_as_written_on_every_arm() {
    use baud_render::{FaceRole, FaceStyle};
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let (regular, italic) = (
        read("baud-app/fonts/built/Regular.ttf"),
        read("baud-app/fonts/built/Italic.ttf"),
    );
    let faces = [
        Face::parse(&regular).expect("parse"),
        Face::parse(&italic).expect("parse"),
    ];
    let roles = [FaceRole::Fallback, FaceRole::Style(FaceStyle::Italic)];
    let renderer = Renderer::new(
        Stack::with_roles(&faces, &roles).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    arms!(renderer => arms, t);
    let slanted =
        "\u{5D0}\u{5D1}\x1b[3m\x1b[17 ]\u{5D2}\x1b[0m\x1b[0 ]\u{5D3}\u{5D4}\u{5D5}\x1b[4D";
    for (scene, text, pen) in [
        ("\x1b[2 ]12345\x1b[4D", "xyz", ""),
        (slanted, "\u{5D5}\u{5D5}", "\x1b[3m\x1b[17 ]"),
    ] {
        feed(
            &mut t,
            b"\x1bc\x1b[2 q\x1b[8;3HThe quick brown fox jumps 0123456789\x1b[3;1H",
        );
        feed(&mut t, scene.as_bytes());
        let cps: Vec<_> = text
            .chars()
            .map(baud_domain::Codepoint::from_char)
            .collect();
        arms.composition = Some(Composition::at_cursor(&t, &cps, &[], Caret::End));
        let composed = three_arms(&mut arms, &mut t, "composed");
        arms.composition = None;
        let plain = three_arms(&mut arms, &mut t, "plain");
        feed(&mut t, format!("{pen}\x1b[4m{text}\x1b[24m").as_bytes());
        let written = three_arms(&mut arms, &mut t, "written");
        assert!(differing(&composed, &plain) > 0, "{text:?}: drawn");
        let d = differing(&composed, &written);
        assert_eq!(d, 0, "{text:?}: as written, {d} pixels");
    }
}

/// **A predicted echo on a right-to-left row draws as its echo will**: the row is
/// laid out with the predicted character in its cell, so `c` typed after `ab` in a
/// Hebrew paragraph is drawn right of the `b`, the `ab` moved left, and the cursor
/// on the cell after it in logical order, which the paragraph draws left of `abc`
/// -- the frame the shell's echo then draws, pixel for pixel, on the cell path,
/// which draws the predictions. At the cursor's drawn cell it read `cab`. The `ab`
/// is red and so is the echo, as Mosh guesses it: the rendition of the logical cell
/// to its left.
#[test]
fn a_predicted_echo_on_a_right_to_left_row_draws_as_its_echo() {
    let (built, noto) = rtl_faces();
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&noto).expect("parse"),
    ];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let line = "\u{5E9}\u{5DC}\u{5D5}\u{5DD} \x1b[31mab";
    arms!(fresh() => arms, t);
    arms.reads = Reads::Copy;
    feed(
        &mut t,
        format!(
            "\x1bc\x1b[2 q\x1b[3;1H{line}\x1b[4;3HThe quick brown fox jumps 0123456789\x1b[3;8H"
        )
        .as_bytes(),
    );
    let (row, col) = t.cursor();
    let (_, plain) = arms.draw(&mut t);
    arms.predictions = vec![(row, col, baud_domain::Codepoint::from_char('c'))];
    let (_, predicted) = arms.draw(&mut t);
    assert!(
        arms.renderer.bidi().reordered(row),
        "the premise: the row is reordered"
    );
    arms.predictions.clear();
    feed(&mut t, b"c");
    let (_, echoed) = arms.draw(&mut t);
    assert!(differing(&predicted, &plain) > 0, "the prediction is drawn");
    let d = differing(&predicted, &echoed);
    assert_eq!(d, 0, "the prediction is the echo's frame, {d} pixels");
}

/// **A predicted bracket on a right-to-left row draws mirrored, as its
/// echo does**: `(` or `[` typed after a Hebrew letter at the end of a
/// right-to-left paragraph resolves to R, and UAX #9 L4
/// (`[[source: uax9-bidi]]`) depicts a character "by a mirrored glyph if
/// and only if (a) the resolved directionality of that character is R, and
/// (b) the Bidi_Mirrored property value of that character is Yes". So the
/// prediction is the character the row's order draws in its cell, `)` or
/// `]`, and its frame is the echo's, pixel for pixel, on the cell path.
/// The premise: on both frames the order swaps that cell's character.
/// The control: a `c` at the same cell, which nothing mirrors.
#[test]
fn a_predicted_bracket_on_a_right_to_left_row_draws_mirrored_as_its_echo() {
    let (built, noto) = rtl_faces();
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&noto).expect("parse"),
    ];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let line = "\u{5E9}\u{5DC}\u{5D5}\u{5DD} \u{5D0}";
    for (ch, mirrored) in [('(', true), ('[', true), ('c', false)] {
        arms!(fresh() => arms, t);
        arms.reads = Reads::Copy;
        feed(
            &mut t,
            format!(
                "\x1bc\x1b[2 q\x1b[3;1H{line}\x1b[4;3HThe quick brown fox jumps 0123456789\x1b[3;7H"
            )
            .as_bytes(),
        );
        let (row, col) = t.cursor();
        let (_, plain) = arms.draw(&mut t);
        arms.predictions = vec![(row, col, baud_domain::Codepoint::from_char(ch))];
        let (_, predicted) = arms.draw(&mut t);
        // The order the cell path lays the row out in, the one that draws
        // the predictions.
        let swapped = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>| {
            arms.with_cells(t, |_, r, _| {
                let orders = r.bidi();
                (
                    orders.reordered(row),
                    orders.swapped(row, orders.visual_col(row, col)).is_some(),
                )
            })
        };
        assert_eq!(
            swapped(&mut arms, &mut t),
            (true, mirrored),
            "{ch:?}: the premise: the row is reordered, and the predicted frame's order \
             mirrors it"
        );
        arms.predictions.clear();
        feed(&mut t, ch.to_string().as_bytes());
        let (_, echoed) = arms.draw(&mut t);
        assert_eq!(
            swapped(&mut arms, &mut t),
            (true, mirrored),
            "{ch:?}: the premise: the echo's order mirrors it"
        );
        assert!(
            differing(&predicted, &plain) > 0,
            "{ch:?}: the prediction is drawn"
        );
        let d = differing(&predicted, &echoed);
        assert_eq!(
            d, 0,
            "{ch:?}: the prediction is the echo's frame, {d} pixels"
        );
    }
}

/// **A paragraph whose first strong letter is Hebrew runs right to left
/// against the right edge, alike on every arm**: UAX #9 P2-P3 give the
/// reported sentence level 1, so its closing period is drawn at the
/// left end of its text and the text ends at the window's right edge,
/// where the erased cells past it go before it
/// (`[[source: terminal-wg-bidi-erased-cells]]`). The expected frame is
/// the same characters written already in visual order, at the right
/// edge, with BDSM reset; a line whose first strong letter is Latin,
/// the control, stays at the left edge on both.
#[test]
fn a_right_to_left_paragraph_is_drawn_against_the_right_edge_on_every_arm() {
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let built = read("baud-app/fonts/built/Regular.ttf");
    let noto = read("baud-font/tests/fonts/NotoColorEmoji-subset.ttf");
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&noto).expect("parse"),
    ];
    let cols = usize::from(WS.ws_col);
    let sentence = "\u{5E9}\u{5DC}\u{5D5}\u{5DD} \u{5E2}\u{5D5}\u{5DC}\u{5DD}! \u{5D6}\u{5D5} \u{5E9}\u{5D5}\u{5E8}\u{5D4} \u{5D1}\u{5E2}\u{5D1}\u{5E8}\u{5D9}\u{5EA}.";
    let n = sentence.chars().count();
    let reversed: String = sentence.chars().rev().collect();
    let control = "hello world, 123 \u{5E9}\u{5DC}\u{5D5}\u{5DD}";
    let control_visual = "hello world, 123 \u{5DD}\u{5D5}\u{5DC}\u{5E9}";
    let three = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, what: &str| {
        arms.reads = Reads::Copy;
        let (a, _) = arms.draw(t);
        let b = arms.draw_copied(t);
        let c = cpu_frame(arms, t);
        assert_eq!(differing(&a, &b), 0, "{what}: the cell arm differs");
        assert_eq!(differing(&a, &c), 0, "{what}: the CPU arm differs");
        a
    };
    let base = b"\x1b[?25l\x1b[2J\x1b[8;1HThe quick brown fox jumps 0123456789";
    arms!(Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(cols),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap() => arms, t);
    feed(&mut t, base);
    feed(
        &mut t,
        format!("\x1b[2;1H{sentence}\x1b[4;1H{control}").as_bytes(),
    );
    let got = three(&mut arms, &mut t, "implicit");
    assert!(arms.renderer.bidi().reordered(Row::from_index(1)));
    assert_eq!(
        arms.renderer
            .bidi()
            .visual_col(Row::from_index(1), Col::at_index(n - 1)),
        Col::at_index(cols - n),
        "the period, written last, drawn at the text's left end"
    );
    feed(&mut t, b"\x1b[8l");
    feed(&mut t, base);
    feed(
        &mut t,
        format!(
            "\x1b[2;{}H{reversed}\x1b[4;1H{control_visual}",
            cols - n + 1
        )
        .as_bytes(),
    );
    let want = three(&mut arms, &mut t, "written in visual order");
    assert_eq!(
        differing(&got, &want),
        0,
        "drawn right to left, at the right"
    );
    feed(&mut t, base);
    feed(
        &mut t,
        format!("\x1b[2;1H{sentence}\x1b[4;1H{control}").as_bytes(),
    );
    let stored = three(&mut arms, &mut t, "explicit");
    assert!(
        differing(&stored, &want) > 0,
        "the control: BDSM reset draws the stored order"
    );
}

/// **A table row keeps its columns, and each cell its own direction, alike
/// on every arm**: the reported table (2026-10-24), its rows drawn with
/// box-drawing rules. Each cell is a paragraph of its own (UAX #9 section
/// 3's table cells, HL4), so the Hebrew name and the Hebrew owner read
/// right to left against their cells' right sides and every rule stays
/// under the one above. The expected frame is the rows written already in
/// that order with BDSM reset.
#[test]
fn a_table_row_keeps_its_columns_on_every_arm() {
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let built = read("baud-app/fonts/built/Regular.ttf");
    let noto = read("baud-font/tests/fonts/NotoColorEmoji-subset.ttf");
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&noto).expect("parse"),
    ];
    let rows = [
        "\u{2502} Name          \u{2502} Size \u{2502} Owner \u{2502}",
        "\u{2502} notes.txt     \u{2502} 42   \u{2502} dev   \u{2502}",
        "\u{2502} \u{5DE}\u{5E1}\u{5DE}\u{5DA}.txt      \u{2502} 812  \u{2502} \u{5D3}\u{5E0}\u{5D4}   \u{2502}",
    ];
    let visual = "\u{2502}      txt.\u{5DA}\u{5DE}\u{5E1}\u{5DE} \u{2502} 812  \u{2502}   \u{5D4}\u{5E0}\u{5D3} \u{2502}";
    let three = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, what: &str| {
        arms.reads = Reads::Copy;
        let (a, _) = arms.draw(t);
        let b = arms.draw_copied(t);
        let c = cpu_frame(arms, t);
        assert_eq!(differing(&a, &b), 0, "{what}: the cell arm differs");
        assert_eq!(differing(&a, &c), 0, "{what}: the CPU arm differs");
        a
    };
    let base = b"\x1b[?25l\x1b[2J\x1b[8;1HThe quick brown fox jumps 0123456789";
    arms!(Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap() => arms, t);
    feed(&mut t, base);
    for (i, r) in rows.iter().enumerate() {
        feed(&mut t, format!("\x1b[{};1H{r}", i + 2).as_bytes());
    }
    let got = three(&mut arms, &mut t, "implicit");
    assert!(arms.renderer.bidi().reordered(Row::from_index(3)));
    feed(&mut t, b"\x1b[8l");
    feed(&mut t, base);
    for (i, r) in [rows[0], rows[1], visual].iter().enumerate() {
        feed(&mut t, format!("\x1b[{};1H{r}", i + 2).as_bytes());
    }
    let want = three(&mut arms, &mut t, "written in visual order");
    assert_eq!(differing(&got, &want), 0, "each cell in its own direction");
}

/// **A right-to-left paragraph's frames drawn by rows are the whole
/// frame's pixels, on both GPU paths**: its text sits past the extent the
/// core records for the row, at the right edge, so a frame that typed one
/// more letter, erased one, or moved the cursor along the row must still
/// draw where the text is drawn. Both paths draw the cursor's frames by
/// rows.
#[test]
fn a_right_to_left_paragraph_drawn_by_rows_is_the_whole_frame() {
    let sentence = "\u{5E9}\u{5DC}\u{5D5}\u{5DD} \u{5E2}\u{5D5}\u{5DC}\u{5DD}! \u{5D6}\u{5D5} \u{5E9}\u{5D5}\u{5E8}\u{5D4} \u{5D1}\u{5E2}\u{5D1}\u{5E8}\u{5D9}\u{5EA}.";
    let steps = |row: Want, rows: Want| -> Vec<Step> {
        let inc = Damage::Incremental;
        vec![
            (
                "the scene and the sentence",
                inc,
                Box::new(move |_, t| {
                    feed(
                        t,
                        b"\x1b[?25h\x1b[2J\x1b[8;1HThe quick brown fox jumps 0123456789",
                    );
                    feed(t, format!("\x1b[6;1H{sentence}").as_bytes());
                }),
                Want::Any,
            ),
            (
                "one more letter",
                inc,
                Box::new(|_, t| feed(t, "\u{5D0}".as_bytes())),
                row,
            ),
            (
                "and erased again",
                inc,
                Box::new(|_, t| feed(t, b"\x08\x1b[K")),
                row,
            ),
            ("the cursor along the row", inc, at(6, 3), rows),
            ("and to the period", inc, at(6, 26), rows),
        ]
    };
    cell_oracle(Reads::Copy, Px::of(0), steps(Want::Any, Want::Rows), 0);
    instance_oracle(Px::of(0), steps(Want::Any, Want::Rows), 0);
}

/// **The cursor on a right-to-left row is drawn in its own shape, alike on
/// every arm**: a block over its cell, an underline under it, and a bar on
/// the cell's right edge where the character under it resolves right to
/// left (`[[source: terminal-wg-bidi-cursor]]`'s "at the very least ... the
/// right side"), wherever the order draws that cell. On an ordinary row,
/// and on a full row in the pending-wrap state, where the cursor's cell is
/// the row's last logical column: drawn, where terminal-wg prefers hiding
/// it. A full row of Latin letters is the control, its cursor on its last
/// visual column with the bar on the left edge.
#[test]
fn the_cursor_on_a_right_to_left_row_is_drawn_in_its_shape_on_every_arm() {
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let built = read("baud-app/fonts/built/Regular.ttf");
    let noto = read("baud-font/tests/fonts/NotoColorEmoji-subset.ttf");
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&noto).expect("parse"),
    ];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let cols = usize::from(WS.ws_col);
    // A full row: as many letters as columns, the last one written leaving
    // the cursor in the pending-wrap state on the last logical column.
    let full = |first: u32, n: u32| -> String {
        (0..cols)
            .map(|i| char::from_u32(first + u32::try_from(i).unwrap() % n).unwrap())
            .collect()
    };
    let hebrew = full(0x5D0, 27);
    let latin = full(u32::from('a'), 26);
    // (what, the scene with the cursor placed, the row, the visual column
    // it must be drawn over, whether its character runs right to left)
    let ordinary = "\x1b[2;3Habc \u{5E9}\u{5DC}\u{5D5}\u{5DD} def\x1b[2;8H".to_string();
    let scenes = [
        ("an ordinary row", ordinary, 1, 8, true),
        ("a full row", format!("\x1b[4;1H{hebrew}"), 3, 0, true),
        (
            "a full row of Latin",
            format!("\x1b[4;1H{latin}"),
            3,
            cols - 1,
            false,
        ),
    ];
    let three = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, what: &str| {
        arms.reads = Reads::Copy;
        let (a, _) = arms.draw(t);
        let b = arms.draw_copied(t);
        let c = cpu_frame(arms, t);
        assert_eq!(differing(&a, &b), 0, "{what}: the cell arm differs");
        assert_eq!(differing(&a, &c), 0, "{what}: the CPU arm differs");
        a
    };
    for (what, text, row, visual, rtl) in &scenes {
        for (style, shape) in [("2", "block"), ("4", "underline"), ("6", "bar")] {
            let at = format!("{what}, {shape}");
            arms!(fresh() => arms, t);
            // The glyphs the harness requires of a scene, on a row of
            // their own, then the scene.
            feed(
                &mut t,
                b"\x1b[?25l\x1b[2J\x1b[8;1HThe quick brown fox jumps 0123456789",
            );
            feed(&mut t, format!("\x1b[{style} q{text}").as_bytes());
            assert_eq!(t.cursor().0, Row::from_index(*row), "{at}");
            if what.starts_with("a full row") {
                assert_eq!(t.wrap_pending(), WrapPending::Yes, "{at}: the premise");
                assert_eq!(t.cursor().1, Col::at_index(cols - 1), "{at}");
            }
            let hidden = three(&mut arms, &mut t, &format!("{at}, hidden"));
            feed(&mut t, b"\x1b[?25h");
            if what.starts_with("a full row") {
                assert_eq!(t.wrap_pending(), WrapPending::Yes, "{at}: still pending");
            }
            let shown = three(&mut arms, &mut t, &at);
            assert_eq!(
                arms.renderer
                    .bidi()
                    .visual_col(Row::from_index(*row), t.cursor().1),
                Col::at_index(*visual),
                "{at}: the cell's drawn column"
            );
            let w = usize::try_from(arms.renderer.whole_size().0.value()).unwrap();
            let cw = usize::try_from(arms.renderer.metrics().cell_w.value()).unwrap();
            let ch = usize::try_from(arms.renderer.metrics().cell_h.value()).unwrap();
            let band = row * ch..(row + 1) * ch;
            let changed: Vec<(usize, usize)> = (0..shown.len())
                .filter(|&i| shown[i] != hidden[i])
                .map(|i| (i % w, i / w))
                .collect();
            assert!(!changed.is_empty(), "{at}: drawn");
            let (left, right) = (visual * cw, (visual + 1) * cw);
            assert!(
                changed
                    .iter()
                    .all(|&(x, y)| x >= left && x < right && band.contains(&y)),
                "{at}: inside visual column {visual} of row {row}"
            );
            let xs = |x: usize| changed.iter().any(|&(cx, _)| cx == x);
            match shape {
                "block" => assert!(xs(left) && xs(right - 1), "{at}: the block covers its cell"),
                "underline" => {
                    assert!(xs(left) && xs(right - 1), "{at}: across its cell");
                    assert!(
                        changed.iter().all(|&(_, y)| y >= band.start + ch / 2),
                        "{at}: under it, in the cell's lower half"
                    );
                }
                _ => {
                    let (on, off) = match rtl {
                        true => (right - 1, left),
                        false => (left, right - 1),
                    };
                    assert!(xs(on), "{at}: the bar on the side its letter runs from");
                    assert!(!xs(off), "{at}: and not on the other");
                }
            }
        }
    }
}

/// **Arabic letters are drawn joined, alike on every arm**
/// (`[[source: unicode-ch9]]` 9.2.2: "a display process must select an
/// appropriate glyph to depict each Arabic letter according to its
/// immediate joining context"), on Courier New, the face macOS gives
/// Arabic beside JetBrains Mono, whose `GSUB` forms are the glyphs its
/// `cmap` maps the presentation forms to (`baud-font/tests/arabic.rs`
/// asserts it glyph for glyph).
///
/// So the expected frame is the presentation forms written in their visual
/// order with BDSM reset, which the paths draw as they drew any character
/// before joining existed: each word with BDSM set, in its logical order,
/// must draw exactly that on the instance arm, the cell arm and the CPU
/// arm. A row whose first strong letter is Arabic runs right to left
/// against the right edge (UAX #9 P2-P3), so each word written from column
/// 3 is drawn ending two cells from the right edge, and the expected frame
/// writes it there. The words hold a dual-joining run, a right-joining
/// letter that breaks it, a letter carrying a fatha, which the cell arm
/// draws by a record, and a beh made initial by a ZWJ after it. With BDSM
/// reset a word already written in its visual order is joined as it is
/// displayed and draws the same. A lam-alef pair draws the ligature
/// centred over its two cells, alike on every arm.
#[test]
fn arabic_letters_draw_joined_on_every_arm() {
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let built = read("baud-app/fonts/built/Regular.ttf");
    let courier = std::fs::read("/System/Library/Fonts/Supplemental/Courier New.ttf")
        .expect("Courier New, which macOS ships; this test cannot skip itself");
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&courier).expect("parse"),
    ];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    // Each word as stored, and its presentation forms in the order drawn,
    // left to right: "bayt" (teh final, yeh medial, beh initial);
    // "muhammad"; "marhaba", whose reh joins only the meem before it, so
    // the hah after it starts again; beh-fatha-beh; beh with ZWJ, alone and
    // initial.
    let rows: [(&str, &str); 5] = [
        ("\u{0628}\u{064A}\u{062A}", "\u{FE96}\u{FEF4}\u{FE91}"),
        (
            "\u{0645}\u{062D}\u{0645}\u{062F}",
            "\u{FEAA}\u{FEE4}\u{FEA4}\u{FEE3}",
        ),
        (
            "\u{0645}\u{0631}\u{062D}\u{0628}\u{0627}",
            "\u{FE8E}\u{FE92}\u{FEA3}\u{FEAE}\u{FEE3}",
        ),
        ("\u{0628}\u{064E}\u{0628}", "\u{FE90}\u{FE91}\u{064E}"),
        ("\u{0628}\u{200D}", "\u{FE91}"),
    ];
    // The cells a word takes: a fatha and a ZWJ ride on the cell before.
    let cells = |w: &str| {
        w.chars()
            .filter(|&c| c != '\u{064E}' && c != '\u{200D}')
            .count()
    };
    // Column 3 with BDSM set, where its paragraph is laid out from; with it
    // reset, the column the word is drawn from with it set: its last cell
    // two from the right edge, as columns 1 and 2 are drawn there.
    let cols = usize::from(WS.ws_col);
    let scene = |t: &mut Term<'_>, implicit: bool, pick: &dyn Fn(&(&str, &str)) -> String| {
        feed(t, b"\x1b[?25l\x1b[2J\x1b[H");
        feed(t, if implicit { b"\x1b[8h" } else { b"\x1b[8l" });
        for (i, row) in rows.iter().enumerate() {
            let word = pick(row);
            let at = match implicit {
                true => 3,
                false => cols - 1 - cells(&word),
            };
            feed(t, format!("\x1b[{};{at}H{word}", i + 2).as_bytes());
        }
        feed(
            t,
            "\x1b[9;3HThe quick brown fox jumps 0123456789".as_bytes(),
        );
    };
    // Each word as an application writes it in its visual order: the
    // stored letters reversed, a cell's marks kept on it. The ZWJ word is
    // written in its forms: in display order a ZWJ stored after a letter
    // is on its right, which makes it final.
    let displayed = |r: &(&str, &str)| {
        if r.0.contains('\u{200D}') {
            return r.1.to_string();
        }
        let mut cells: Vec<String> = Vec::new();
        for c in r.0.chars() {
            match c {
                '\u{064E}' | '\u{200D}' => cells.last_mut().unwrap().push(c),
                c => cells.push(c.to_string()),
            }
        }
        cells.reverse();
        cells.concat()
    };
    let three = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, what: &str| {
        arms.reads = Reads::Copy;
        let (a, _) = arms.draw(t);
        let b = arms.draw_copied(t);
        let c = cpu_frame(arms, t);
        assert_eq!(differing(&a, &b), 0, "{what}: the cell arm differs");
        assert_eq!(differing(&a, &c), 0, "{what}: the CPU arm differs");
        a
    };
    {
        arms!(fresh() => arms, t);
        scene(&mut t, true, &|r| r.0.to_string());
        let got = three(&mut arms, &mut t, "joined");
        scene(&mut t, false, &|r| r.1.to_string());
        let want = three(&mut arms, &mut t, "the presentation forms");
        assert_eq!(differing(&got, &want), 0, "the joined forms drawn");
        // With BDSM reset, a word an application wrote in its visual order
        // is joined as displayed: the stored letters reversed draw it too.
        scene(&mut t, false, &displayed);
        let displayed = three(&mut arms, &mut t, "joined as displayed");
        assert_eq!(differing(&displayed, &want), 0, "joined in display order");
        // The control: the letters drawn alone are not the forms.
        scene(&mut t, false, &|r| r.0.chars().rev().collect());
        let _ = three(&mut arms, &mut t, "explicit, logical");
        feed(&mut t, b"\x1b[2J");
        for (i, (word, _)) in rows.iter().enumerate() {
            let alone: String = word
                .chars()
                .filter(|&c| c != '\u{064E}' && c != '\u{200D}')
                .flat_map(|c| [c, ' '])
                .collect();
            let at = cols - 1 - cells(&alone);
            feed(&mut t, format!("\x1b[{};{at}H{alone}", i + 2).as_bytes());
        }
        let apart = three(&mut arms, &mut t, "letters apart");
        assert!(
            differing(&apart, &want) > 0,
            "the control: a letter alone is not its form"
        );
    }
    // **The first joining letter a renderer draws, with BDSM reset**: no
    // row is read for joins before one is drawn, so the frame that draws
    // the first lays out again -- the cell arm after its walk, as the app
    // does, the CPU arm by converting again -- and draws it joined. Each
    // arm first, on a renderer that has drawn only the presentation forms,
    // which take no form.
    for first in ["cell", "cpu"] {
        arms!(fresh() => arms, t);
        arms.reads = Reads::Copy;
        let draw = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>| match first {
            // The cell arm, drawn before the instance arm converts.
            "cell" => arms.draw(t).1,
            _ => cpu_frame(arms, t),
        };
        scene(&mut t, false, &|r| r.1.to_string());
        let want = draw(&mut arms, &mut t);
        assert_eq!(
            arms.renderer.joins_seen(),
            baud_render::Joins::Unseen,
            "{first}: the presentation forms take no form"
        );
        scene(&mut t, false, &displayed);
        let got = draw(&mut arms, &mut t);
        assert_eq!(
            differing(&got, &want),
            0,
            "{first}: the first joining letters drawn are joined"
        );
    }
    // **A lam-alef pair**: "salam" (seen, lam, alef, meem) alike on every
    // arm, and "la" alone, whose ligature is the presentation form U+FEFB
    // drawn half a cell right of where a one-cell character draws it:
    // centred over the pair's two cells. Each paragraph is laid out
    // against the right edge: "la" written from column 4 is drawn with its
    // alef in column 116, where U+FEFB is written for the control.
    {
        arms!(fresh() => arms, t);
        let ink = |arms: &Arms<'_, '_>, got: &[u32], blank: &[u32], row: usize| -> (usize, usize) {
            let w = usize::try_from(arms.renderer.whole_size().0.value()).unwrap();
            let ch = usize::try_from(arms.renderer.metrics().cell_h.value()).unwrap();
            let inked: Vec<usize> = (0..w)
                .filter(|&x| (row * ch..(row + 1) * ch).any(|y| got[y * w + x] != blank[y * w + x]))
                .collect();
            (*inked.first().unwrap(), *inked.last().unwrap())
        };
        feed(&mut t, b"\x1b[?25l\x1b[2J\x1b[H\x1b[8h");
        feed(
            &mut t,
            "\x1b[4;3HThe quick brown fox jumps 0123456789".as_bytes(),
        );
        let blank = three(&mut arms, &mut t, "no word");
        feed(
            &mut t,
            "\x1b[2;3H\u{0633}\u{0644}\u{0627}\u{0645}".as_bytes(),
        );
        let _ = three(&mut arms, &mut t, "salam");
        feed(
            &mut t,
            "\x1b[2;1H\x1b[K\x1b[2;4H\u{0644}\u{0627}".as_bytes(),
        );
        let pair = three(&mut arms, &mut t, "la");
        feed(
            &mut t,
            "\x1b[2;1H\x1b[K\x1b[8l\x1b[2;116H\u{FEFB}".as_bytes(),
        );
        let one = three(&mut arms, &mut t, "U+FEFB in one cell");
        let cw = usize::try_from(arms.renderer.metrics().cell_w.value()).unwrap();
        let (lo, hi) = ink(&arms, &pair, &blank, 1);
        let (lo1, hi1) = ink(&arms, &one, &blank, 1);
        let half = cw as f32 / 2.0;
        for (a, b) in [(lo, lo1), (hi, hi1)] {
            assert!(
                ((a as f32 - b as f32) - half).abs() <= 1.0,
                "the pair's ink {lo}..={hi} is U+FEFB's {lo1}..={hi1} moved half of {cw}"
            );
        }
    }
    // **A pair an erased cell or a soft wrap parts** forms the ligature too
    // (L1: the erased cell is transparent). With an erased cell between
    // them, "la" draws U+FEFB centred over the three cells, where a
    // one-cell character draws it in the middle one; with the lam at the
    // margin and its alef wrapped, U+FEFB in the lam's cell and nothing in
    // the alef's. The pair with a gap is drawn against the right edge, its
    // middle cell column 116; the wrapped one's row starts with Latin and
    // is drawn from the left.
    {
        arms!(fresh() => arms, t);
        let w = usize::try_from(arms.renderer.whole_size().0.value()).unwrap();
        let ch = usize::try_from(arms.renderer.metrics().cell_h.value()).unwrap();
        let ink = |got: &[u32], blank: &[u32], row: usize| -> Option<(usize, usize)> {
            let inked: Vec<usize> = (0..w)
                .filter(|&x| (row * ch..(row + 1) * ch).any(|y| got[y * w + x] != blank[y * w + x]))
                .collect();
            Some((*inked.first()?, *inked.last()?))
        };
        let near = |what: &str, got: Option<(usize, usize)>, want: Option<(usize, usize)>| {
            let (Some((lo, hi)), Some((lo1, hi1))) = (got, want) else {
                panic!("{what}: ink {got:?} against {want:?}");
            };
            assert!(
                lo.abs_diff(lo1) <= 1 && hi.abs_diff(hi1) <= 1,
                "{what}: the pair's ink {lo}..={hi} is U+FEFB's {lo1}..={hi1}"
            );
        };
        feed(&mut t, b"\x1b[?25l\x1b[2J\x1b[H\x1b[8h");
        feed(
            &mut t,
            "\x1b[4;3HThe quick brown fox jumps 0123456789".as_bytes(),
        );
        let blank = three(&mut arms, &mut t, "no word");
        feed(&mut t, "\x1b[2;4H\u{0644}\x1b[C\u{0627}".as_bytes());
        let gap = three(&mut arms, &mut t, "la, an erased cell between");
        feed(
            &mut t,
            "\x1b[2;1H\x1b[K\x1b[8l\x1b[2;116H\u{FEFB}".as_bytes(),
        );
        let middle = three(&mut arms, &mut t, "U+FEFB in the middle cell");
        near(
            "an erased cell between",
            ink(&gap, &blank, 1),
            ink(&middle, &blank, 1),
        );
        // The lam in the last column, the alef wrapped to the next row.
        let xs = "x".repeat(usize::from(WS.ws_col) - 1);
        feed(
            &mut t,
            format!("\x1b[2;1H\x1b[K\x1b[8h\x1b[2;1H{xs}").as_bytes(),
        );
        let lined = three(&mut arms, &mut t, "the row before the lam");
        feed(&mut t, "\u{0644}\u{0627}".as_bytes());
        let wrapped = three(&mut arms, &mut t, "la, a soft wrap between");
        feed(
            &mut t,
            format!("\x1b[2;1H\x1b[K\x1b[3;1H\x1b[K\x1b[8l\x1b[2;1H{xs}\u{FEFB}").as_bytes(),
        );
        let lams = three(&mut arms, &mut t, "U+FEFB in the lam's cell");
        // The lam's row is U+FEFB in the lam's cell, pixel for pixel.
        let band = |f: &[u32]| f[ch * w..2 * ch * w].to_vec();
        assert_eq!(
            differing(&band(&wrapped), &band(&lams)),
            0,
            "a soft wrap between: the lam's row"
        );
        assert_eq!(
            ink(&wrapped, &lined, 2),
            None,
            "the alef's cell draws nothing"
        );
    }
    // **Two pairs side by side** -- lam, alef, lam, alef, as in "salalat"
    // -- are two ligatures on every arm: each lam finds its own alef, not
    // the alef of the pair beside it. With BDSM set the word is stored in
    // logical order, from column 4, and drawn against the right edge; with
    // it reset, in display order where that draws it, which draws the same.
    {
        arms!(fresh() => arms, t);
        let mut frames = Vec::new();
        for (bdsm, word, at) in [
            ("\x1b[8h", "\u{0644}\u{0627}\u{0644}\u{0627}", 4),
            ("\x1b[8l", "\u{0627}\u{0644}\u{0627}\u{0644}", 114),
        ] {
            feed(&mut t, format!("\x1b[?25l\x1b[2J\x1b[H{bdsm}").as_bytes());
            feed(
                &mut t,
                "\x1b[4;3HThe quick brown fox jumps 0123456789".as_bytes(),
            );
            feed(&mut t, format!("\x1b[2;{at}H{word}").as_bytes());
            frames.push(three(&mut arms, &mut t, "two pairs side by side"));
        }
        assert_eq!(
            differing(&frames[0], &frames[1]),
            0,
            "two pairs: logical order with BDSM set, display order reset"
        );
    }
    // **After a word wraps on, the three arms agree**: the last beh of a
    // row ends a word until a beh written on the next row, the row the word
    // wraps to, makes it medial, and each arm drawn after the second write
    // must be what a fresh renderer draws of the same screen. This block
    // stays green with the renderer's repaint of a row whose forms moved
    // taken out, so it does not hold that repaint;
    // `a_moved_joining_form_repaints_its_row` does.
    {
        let row1 = format!("{}{}", "x".repeat(117), "\u{0628}\u{0628}\u{0628}");
        let screen = |t: &mut Term<'_>| {
            feed(t, b"\x1b[?25l\x1b[2J\x1b[H\x1b[8h");
            feed(
                t,
                "\x1b[4;3HThe quick brown fox jumps 0123456789".as_bytes(),
            );
            feed(t, format!("\x1b[1;1H{row1}").as_bytes());
        };
        arms!(fresh() => arms, t);
        screen(&mut t);
        let _ = three(&mut arms, &mut t, "a word at the margin");
        // The cursor waits past the margin: the next beh wraps.
        feed(&mut t, "\u{0628}".as_bytes());
        let moved = three(&mut arms, &mut t, "the word wrapped on");
        arms!(fresh() => fresh_arms, u);
        screen(&mut u);
        feed(&mut u, "\u{0628}".as_bytes());
        let fresh_frame = three(&mut fresh_arms, &mut u, "drawn fresh");
        assert_eq!(
            differing(&moved, &fresh_frame),
            0,
            "the moved form is drawn"
        );
    }
}

/// **A letter of a joining script wider than its cell draws alike on every
/// arm**, by rule D (`baud-render/src/fit.rs`): full size into the cells
/// its ink reaches when they are blank, else fitted to its one cell, its
/// marks with it. On `joining-cases.ttf`, whose sad is 1300 units isolated
/// and 950 medial against a cell of 600: a sad beside a reh, which the
/// cell arm's map lists for a record; one beside two blank cells, drawn
/// full size; one carrying a fatha, a marked cell; medial sads joined
/// between behs, joined cells; one beside a wide character; and one in the
/// last column -- each with BDSM set and reset. Then a letter typed into
/// the blank cell a full-size sad reaches, which fits it on every arm,
/// as the cell arm's cache keeps the two drawings apart.
#[test]
fn an_over_wide_letter_draws_alike_on_every_arm() {
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let built = read("baud-app/fonts/built/Regular.ttf");
    let cases = read("baud-font/tests/var/joining-cases.ttf");
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&cases).expect("parse"),
    ];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let rows = [
        "\u{0635}\u{0631}",
        "\u{0635}  \u{0631}",
        "\u{0635}\u{064E}\u{0631}",
        "\u{0628}\u{0635}\u{0635}\u{0628}",
        "\u{0635}\u{4E2D}",
    ];
    let three = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, what: &str| {
        arms.reads = Reads::Copy;
        let (a, _) = arms.draw(t);
        let b = arms.draw_copied(t);
        let c = cpu_frame(arms, t);
        assert_eq!(differing(&a, &b), 0, "{what}: the cell arm differs");
        assert_eq!(differing(&a, &c), 0, "{what}: the CPU arm differs");
        a
    };
    // A line of Latin under the words, so the map holds the glyphs of a
    // screen.
    let fox = "\x1b[9;3HThe quick brown fox jumps 0123456789".as_bytes();
    arms!(fresh() => arms, t);
    for implicit in [true, false] {
        feed(&mut t, b"\x1b[?25l\x1b[2J\x1b[H");
        feed(&mut t, if implicit { b"\x1b[8h" } else { b"\x1b[8l" });
        feed(&mut t, fox);
        for (i, row) in rows.iter().enumerate() {
            feed(&mut t, format!("\x1b[{};3H{row}", i + 2).as_bytes());
        }
        feed(
            &mut t,
            format!("\x1b[8;{}H\u{0635}", usize::from(WS.ws_col)).as_bytes(),
        );
        let drawn = three(
            &mut arms,
            &mut t,
            if implicit { "BDSM set" } else { "BDSM reset" },
        );
        // The control: the same rows with every sad written as a reh draw
        // otherwise, so the frames compared hold the sads.
        feed(&mut t, b"\x1b[2J");
        feed(&mut t, fox);
        for (i, row) in rows.iter().enumerate() {
            let rehs = row.replace('\u{0635}', "\u{0631}");
            feed(&mut t, format!("\x1b[{};3H{rehs}", i + 2).as_bytes());
        }
        let control = three(&mut arms, &mut t, "the control");
        assert!(differing(&drawn, &control) > 0, "the control differs");
    }
    // Full size beside two blanks, then a letter typed where it reached.
    feed(&mut t, b"\x1b[2J\x1b[8l");
    feed(&mut t, fox);
    feed(&mut t, "\x1b[2;3H\u{0635}  \u{0631}".as_bytes());
    let full = three(&mut arms, &mut t, "full size");
    feed(&mut t, "\x1b[2;4H\u{0628}".as_bytes());
    let typed = three(&mut arms, &mut t, "typed beside");
    assert!(
        differing(&full, &typed) > 0,
        "the sad fitted once typed beside"
    );
    feed(&mut t, b"\x1b[2;4H ");
    let again = three(&mut arms, &mut t, "erased again");
    assert_eq!(differing(&full, &again), 0, "full size again");
    // The README's line on Noto Sans Arabic 2.013, where the README's font
    // cache holds it (`scripts/readme-capture.sh`): its letters, fitted
    // and not, its dots placed by `GPOS` and its words in a row of their
    // own, alike on every arm. The cache is found as
    // `baud-render/tests/overwide_letters.rs` and `baud-font/tests/arabic.rs`
    // find it, and its file held to the first's pin, so the three skip or
    // run together.
    const SHA: &str = "bd86ca02f087d7f3c3788ba458fb6b73744c7639ed276b8d870dba6def6c40d0";
    let cache = std::env::var_os("NOTO_CACHE")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_CACHE_HOME")
                .map(|d| std::path::PathBuf::from(d).join("baud-readme"))
        })
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache/baud-readme"))
        });
    if let Some(noto) = cache.and_then(|d| std::fs::read(d.join("NotoSansArabic-Regular.ttf")).ok())
    {
        assert_eq!(
            sha256::sha256_hex(&noto),
            SHA,
            "the cache's file is not the pinned one"
        );
        let faces = [
            Face::parse(&built).expect("parse"),
            Face::parse(&noto).expect("parse"),
        ];
        let fresh = || {
            Renderer::new(
                Stack::new(&faces).unwrap(),
                PxSize::of(14.0).unwrap(),
                Theme::dark(),
                Blend::Native,
                Cols::of_index(usize::from(WS.ws_col)),
                Rows::of_index(usize::from(WS.ws_row)),
            )
            .unwrap()
        };
        arms!(fresh() => arms, t);
        for mode in [b"\x1b[8h", b"\x1b[8l"] {
            feed(&mut t, b"\x1b[?25l\x1b[2J\x1b[H");
            feed(&mut t, mode);
            feed(&mut t, fox);
            feed(
                &mut t,
                "\x1b[2;3H\u{0627}\u{0644}\u{0637}\u{0631}\u{0641}\u{064A}\u{0629} \u{062A}\u{0636}\u{0639} \u{0643}\u{0644} \u{062D}\u{0631}\u{0641} \u{0641}\u{064A} \u{0627}\u{0644}\u{0634}\u{0628}\u{0643}\u{0629}.".as_bytes(),
            );
            feed(&mut t, "\x1b[4;3H\u{0636} \u{0635}\u{064E}".as_bytes());
            let _ = three(&mut arms, &mut t, "Noto Sans Arabic");
        }
    }
}

/// **A joined letter is drawn with the letters of its word around it,
/// alike on every arm**: a face's contextual lookups across letters read
/// the cells before and after it in logical order (`rlig`, `rclt`, `calt`;
/// `[[source: ms-opentype-features-pt]]`), and Syriac's alaph takes its
/// own three forms (`[[source: unicode-ch9]]` 9.3, S1-S3). On Noto Sans
/// Syriac, which macOS ships, an alaph's `rlig` picks a wider final after
/// a beth than after a yudh: each word written with an erased cell between
/// its two letters, which joining passes through and the word leaves out,
/// so neither letter's ink is in the other's cell, the alaph's cell draws
/// differently after the two, and so does an alaph a soft wrap parts
/// from a beth and from a yudh. Alaph after he (fin2), after
/// dalath (fin3) and alone draws alike on every arm. When the letter before a
/// wrapped alaph changes from yudh to beth, its form stays final and only
/// its word changes, and each arm draws what a fresh renderer draws. On SF
/// Arabic, macOS's own Arabic face, whose `rclt` takes dotted letters
/// apart into skeletons and dots, the three arms agree, and alef, lam, lam
/// and heh with no shadda draw four letters, not its `rlig`'s word Allah
/// across their cells.
#[test]
fn a_joined_letter_reads_its_word_on_every_arm() {
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let built = read("baud-app/fonts/built/Regular.ttf");
    let syriac = std::fs::read("/System/Library/Fonts/Supplemental/NotoSansSyriac-Regular.ttf")
        .expect("Noto Sans Syriac, which macOS ships; this test cannot skip itself");
    let arabic = std::fs::read("/System/Library/Fonts/SFArabic.ttf")
        .expect("SF Arabic, which macOS ships; this test cannot skip itself");
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&syriac).expect("parse"),
        Face::parse(&arabic).expect("parse"),
    ];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let three = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, what: &str| {
        arms.reads = Reads::Copy;
        let (a, _) = arms.draw(t);
        let b = arms.draw_copied(t);
        let c = cpu_frame(arms, t);
        assert_eq!(differing(&a, &b), 0, "{what}: the cell arm differs");
        assert_eq!(differing(&a, &c), 0, "{what}: the CPU arm differs");
        a
    };
    // The pixels of the cell drawn at logical column `col` of row `row`
    // (both from zero), wherever the layout draws it.
    let crop = |arms: &Arms<'_, '_>, frame: &[u32], row: usize, col: usize| -> Vec<u32> {
        let w = usize::try_from(arms.renderer.whole_size().0.value()).unwrap();
        let cw = usize::try_from(arms.renderer.metrics().cell_w.value()).unwrap();
        let ch = usize::try_from(arms.renderer.metrics().cell_h.value()).unwrap();
        let v = arms
            .renderer
            .bidi()
            .visual_col(Row::from_index(row), Col::at_index(col))
            .index();
        (row * ch..(row + 1) * ch)
            .flat_map(|y| frame[y * w + v * cw..y * w + (v + 1) * cw].to_vec())
            .collect()
    };
    arms!(fresh() => arms, t);
    // A letter, an erased cell and an alaph written from column 3: the
    // alaph's logical column is 4 from zero; an alaph alone, 2.
    let alaph_of = |t: &mut Term<'_>, arms: &mut Arms<'_, '_>, word: &str, what: &str| {
        feed(t, b"\x1b[?25l\x1b[2J\x1b[H\x1b[8h");
        feed(
            t,
            "\x1b[8;3HThe quick brown fox jumps 0123456789".as_bytes(),
        );
        feed(t, format!("\x1b[2;3H{word}").as_bytes());
        let frame = three(arms, t, what);
        let col = match word.chars().count() {
            1 => 2,
            _ => 4,
        };
        crop(arms, &frame, 1, col)
    };
    let after_beth = alaph_of(&mut t, &mut arms, "\u{0712}\x1b[C\u{0710}", "beth, alaph");
    let after_yudh = alaph_of(&mut t, &mut arms, "\u{071D}\x1b[C\u{0710}", "yudh, alaph");
    assert_ne!(
        after_beth, after_yudh,
        "the alaph's rlig reads the letter before it"
    );
    // fin2, fin3 and the isolated form, on every arm: Noto Sans Syriac's
    // Estrangela draws the three alike, so the glyphs are held to
    // hb-shape's in baud-font's `contextual.txt` and not here.
    for (word, what) in [
        ("\u{0717}\x1b[C\u{0710}", "he, alaph"),
        ("\u{0715}\x1b[C\u{0710}", "dalath, alaph"),
        ("\u{0710}", "alaph"),
    ] {
        let _ = alaph_of(&mut t, &mut arms, word, what);
    }
    // Across a soft wrap: the beth in the last column of row 4, its alaph
    // wrapped to row 5.
    let wrapped = |t: &mut Term<'_>, arms: &mut Arms<'_, '_>, first: &str, what: &str| {
        feed(t, b"\x1b[?25l\x1b[2J\x1b[H\x1b[8h");
        feed(
            t,
            "\x1b[8;3HThe quick brown fox jumps 0123456789".as_bytes(),
        );
        feed(t, format!("\x1b[4;120H{first}\u{0710}").as_bytes());
        let frame = three(arms, t, what);
        crop(arms, &frame, 4, 0)
    };
    // The wrapped alaph is drawn in the last column, where rule D fits it
    // to its cell (`baud-render/src/fit.rs`; its ink is 849 units after a
    // yudh and 913 after a beth, uni0710.fina.wideX, each over 1.1 cells
    // of 600), so it is held to the alaph wrapped after a yudh, as the
    // alaph after an erased cell is above.
    assert_ne!(
        wrapped(&mut t, &mut arms, "\u{0712}", "beth, a soft wrap, alaph"),
        wrapped(&mut t, &mut arms, "\u{071D}", "yudh, a soft wrap, alaph"),
        "the word reaches across the wrap"
    );
    // The letter before a wrapped alaph changed: its form stays final, its
    // word does not.
    {
        let screen = |t: &mut Term<'_>, first: &str| {
            feed(t, b"\x1b[?25l\x1b[2J\x1b[H\x1b[8h");
            feed(
                t,
                "\x1b[8;3HThe quick brown fox jumps 0123456789".as_bytes(),
            );
            feed(t, format!("\x1b[4;120H{first}\u{0710}").as_bytes());
        };
        arms!(fresh() => arms, t);
        screen(&mut t, "\u{071D}");
        let _ = three(&mut arms, &mut t, "yudh, a soft wrap, alaph");
        feed(&mut t, "\x1b[4;120H\u{0712}".as_bytes());
        let moved = three(&mut arms, &mut t, "the yudh made a beth");
        arms!(fresh() => fresh_arms, u);
        screen(&mut u, "\u{0712}");
        let fresh_frame = three(&mut fresh_arms, &mut u, "drawn fresh");
        assert_eq!(
            differing(&moved, &fresh_frame),
            0,
            "the alaph's changed word is drawn"
        );
    }
    // SF Arabic's rlig would take alef, lam, lam and heh with no shadda
    // into the word Allah, one glyph across four cells, which 9.2.11 says
    // should not form: each cell draws its own letter, on every arm.
    {
        feed(&mut t, b"\x1b[?25l\x1b[2J\x1b[H\x1b[8h");
        feed(
            &mut t,
            "\x1b[8;3HThe quick brown fox jumps 0123456789".as_bytes(),
        );
        let blank = three(&mut arms, &mut t, "no word");
        feed(
            &mut t,
            "\x1b[2;3H\u{0627}\u{0644}\u{0644}\u{0647}".as_bytes(),
        );
        let word = three(&mut arms, &mut t, "alef, lam, lam, heh");
        for col in 2..6 {
            assert_ne!(
                crop(&arms, &word, 1, col),
                crop(&arms, &blank, 1, col),
                "logical column {col}: the letter draws in its own cell"
            );
        }
    }
    // SF Arabic's rclt, on every arm: behs taken apart, a feh and alef
    // maqsura picking their short forms, and a lam-alef pair in a word.
    for word in [
        "\u{0628}\u{0628}\u{0628}",
        "\u{0639}\u{0631}\u{0628}\u{064A}",
        "\u{0641}\u{0649}",
        "\u{0628}\u{0644}\u{0627}\u{0628}",
    ] {
        feed(&mut t, b"\x1b[?25l\x1b[2J\x1b[H\x1b[8h");
        feed(
            &mut t,
            "\x1b[8;3HThe quick brown fox jumps 0123456789".as_bytes(),
        );
        feed(&mut t, format!("\x1b[2;3H{word}").as_bytes());
        let _ = three(&mut arms, &mut t, word);
    }
}

/// The pixels of the cell drawn at logical column `col` of row `row` of
/// `frame` (both from zero), wherever the layout draws it.
fn crop_cell(arms: &Arms<'_, '_>, frame: &[u32], (row, col): (usize, usize)) -> Vec<u32> {
    let w = usize::try_from(arms.renderer.whole_size().0.value()).unwrap();
    let cw = usize::try_from(arms.renderer.metrics().cell_w.value()).unwrap();
    let ch = usize::try_from(arms.renderer.metrics().cell_h.value()).unwrap();
    let v = arms
        .renderer
        .bidi()
        .visual_col(Row::from_index(row), Col::at_index(col))
        .index();
    (row * ch..(row + 1) * ch)
        .flat_map(|y| frame[y * w + v * cw..y * w + (v + 1) * cw].to_vec())
        .collect()
}

/// `t` drawn on the instance, cell and CPU arms, which must agree.
fn three_arms(arms: &mut Arms<'_, '_>, t: &mut Term<'_>, what: &str) -> Vec<u32> {
    arms.reads = Reads::Copy;
    let (a, _) = arms.draw(t);
    let b = arms.draw_copied(t);
    let c = cpu_frame(arms, t);
    assert_eq!(differing(&a, &b), 0, "{what}: the cell arm differs");
    assert_eq!(differing(&a, &c), 0, "{what}: the CPU arm differs");
    a
}

/// **Text enough that the map holds the scene's glyphs**, which
/// [`Arms::draw`] asks of every frame: a line on row 7, which no message,
/// address or line check reads, written with the cursor saved and put back,
/// so a test's cursor is where it left it.
const ENOUGH_GLYPHS: &[u8] = b"\x1b7\x1b[7;3HThe quick brown fox jumps 0123456789\x1b8";

/// **baud's message pair on every arm**: the instance arm, the cell arm
/// through `Overlay::composed` and the CPU arm draw one frame, and under
/// each of a program's `OSC 17`, `19`, `10`, `11`, `4` and `?5`, on dark and
/// light, its padding is `#F4A32C` in every pixel and its letters reach
/// `#15171B`; with both colours `"none"` and a program's unreadable pair, the
/// same pair at draw. Perturbations: read `Hints::MESSAGE` as the selection
/// in `Drawing::of`, which any program moves on all three; drop the page in
/// `composition_parts`' `look.pair` arm, which shows the frame through the
/// box on the instance and cell arms; resolve in `in_force` with no check,
/// which draws the grey pair.
#[test]
fn the_message_pair_holds_on_every_arm() {
    use baud_render::{MessageAt, MessageColours, MessagePaint, Pair, TipAnchor, message_look};
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let built = read("baud-app/fonts/built/Regular.ttf");
    let faces = [Face::parse(&built).expect("parse")];
    let (cols, rows) = (
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    );
    let fresh = |theme: Theme| {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            theme,
            Blend::Native,
            cols,
            rows,
        )
        .unwrap()
    };
    let at = (Row::from_index(3), Col::at_index(5));
    let look = message_look(
        "Copied 42 characters",
        cols,
        rows,
        MessageAt::Tip(TipAnchor {
            first: at,
            last: at,
        }),
    )
    .expect("room");
    let (row, left) = (look.lines[0].row.index(), look.lines[0].col.index());
    let width = look.lines[0].text.len();
    let none = MessageColours {
        page: MessagePaint::Selection,
        text: MessagePaint::Selection,
    };
    let programs: [(&str, &[u8], MessageColours); 8] = [
        ("nothing", b"", MessageColours::default()),
        (
            "OSC 17",
            b"\x1b]17;#102030\x1b\\",
            MessageColours::default(),
        ),
        (
            "OSC 19",
            b"\x1b]19;#a0a0a0\x1b\\",
            MessageColours::default(),
        ),
        (
            "OSC 10",
            b"\x1b]10;#00ff00\x1b\\",
            MessageColours::default(),
        ),
        (
            "OSC 11",
            b"\x1b]11;#ff00ff\x1b\\",
            MessageColours::default(),
        ),
        (
            "OSC 4",
            b"\x1b]4;0;#ff0000;7;#0000ff;15;#ffff00\x1b\\",
            MessageColours::default(),
        ),
        ("?5", b"\x1b[?5h", MessageColours::default()),
        (
            "\"none\" under 4.5:1",
            b"\x1b]17;#808080\x1b\\\x1b]19;#909090\x1b\\",
            none,
        ),
    ];
    for theme in [Theme::dark(), Theme::light()] {
        for (name, bytes, colours) in programs {
            arms!(fresh(theme.clone()) => arms, t);
            feed(&mut t, b"\x1b[?25lsome text under it\r\nand more");
            feed(&mut t, ENOUGH_GLYPHS);
            feed(&mut t, bytes);
            arms.renderer.set_message_colours(colours);
            arms.tip = Some(look.clone());
            let frame = three_arms(&mut arms, &mut t, name);
            for p in crop_cell(&arms, &frame, (row, left)) {
                assert_eq!(p, Pair::BAUD.page.0, "{name}: the box, whole");
            }
            let reached = (left + 1..left + width - 1)
                .flat_map(|col| crop_cell(&arms, &frame, (row, col)))
                .any(|p| p == Pair::BAUD.text.0);
            assert!(reached, "{name}: the text's own colour was drawn");
        }
    }
}

/// A program's colours in every form they take, for the address and the
/// notice: what each of these moves, the selection's colours among them.
const COLOURS_A_PROGRAM_SETS: [(&str, &[u8]); 7] = [
    ("nothing", b""),
    ("OSC 17", b"\x1b]17;#102030\x1b\\"),
    ("OSC 19", b"\x1b]19;#102030\x1b\\"),
    ("OSC 10", b"\x1b]10;#00ff00\x1b\\"),
    ("OSC 11", b"\x1b]11;#ff00ff\x1b\\"),
    ("OSC 4", b"\x1b]4;0;#ff0000;7;#0000ff;15;#ffff00\x1b\\"),
    ("?5", b"\x1b[?5h"),
];

/// The address bar's look on the bottom row, in the message pair, as
/// `baud-app`'s `link::preview` builds it.
fn address_look(address: &str) -> baud_render::LinkLook {
    let text: Vec<_> = format!(" {address} ")
        .chars()
        .map(baud_domain::Codepoint::from_char)
        .collect();
    let n = text.len();
    baud_render::LinkLook {
        spans: Vec::new(),
        bar: baud_render::Field {
            row: Row::from_index(usize::from(WS.ws_row) - 1),
            col: Col::ZERO,
            text,
            hints: vec![baud_render::Hints::MESSAGE; n],
            caret: Col::at_index(n),
        },
    }
}

/// **The address and the config's notice on every arm, whatever a program
/// sets**: the deciding test of the bar's colours. Under each of `OSC 17`,
/// `19`, `10`, `11`, `4` and `?5`, and with the selection's colours made to
/// match, the instance arm, the cell arm through `Overlay::composed` and the
/// CPU arm draw one frame: the bar's and the notice's padding cells are
/// `#F4A32C` in every pixel and their letters reach `#15171B`. Perturbations:
/// build the bar with `Hints::SELECTION` in `link::preview` (and here); build
/// the notice with `Hints::SELECTION` in `Renderer::notice_field`; each moves
/// with the program on all three arms.
#[test]
fn the_address_and_the_notice_take_no_colour_from_a_program() {
    use baud_render::Pair;
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let built = read("baud-app/fonts/built/Regular.ttf");
    let faces = [Face::parse(&built).expect("parse")];
    let (cols, rows) = (
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    );
    let fresh = |theme: Theme| {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            theme,
            Blend::Native,
            cols,
            rows,
        )
        .unwrap()
    };
    let look = address_look("https://example.org/docs");
    let width = look.bar.text.len();
    let last = usize::from(WS.ws_row) - 1;
    let notice: Vec<baud_domain::Codepoint> = "baud: a setting was not read"
        .chars()
        .map(baud_domain::Codepoint::from_char)
        .collect();
    for theme in [Theme::dark(), Theme::light()] {
        for (name, bytes) in COLOURS_A_PROGRAM_SETS {
            arms!(fresh(theme.clone()) => arms, t);
            feed(&mut t, b"\x1b[?25lsome text under it\r\nand more");
            feed(&mut t, ENOUGH_GLYPHS);
            feed(&mut t, bytes);
            arms.link = Some(look.clone());
            let frame = three_arms(&mut arms, &mut t, name);
            for p in crop_cell(&arms, &frame, (last, 0)) {
                assert_eq!(p, Pair::BAUD.page.0, "{name}: the bar's box, whole");
            }
            assert!(
                (1..width - 1)
                    .flat_map(|col| crop_cell(&arms, &frame, (last, col)))
                    .any(|p| p == Pair::BAUD.text.0),
                "{name}: the address's own colour was drawn"
            );
            // The notice, the first frame of a renderer of its own, its row the
            // one the terminal gave up.
            arms!(fresh(theme.clone()) => arms, t);
            feed(&mut t, b"\x1b[?25lsome text under it\r\nand more");
            feed(&mut t, ENOUGH_GLYPHS);
            feed(&mut t, bytes);
            let _ = t.resize(&WinSize {
                ws_col: WS.ws_col,
                ws_row: WS.ws_row - 1,
            });
            arms.renderer.set_notice(Some(notice.clone()));
            arms.renderer.fit(&t);
            let frame = three_arms(&mut arms, &mut t, name);
            let row = usize::from(WS.ws_row) - 1;
            let blank = notice.len() + 2;
            for p in crop_cell(&arms, &frame, (row, blank)) {
                assert_eq!(p, Pair::BAUD.page.0, "{name}: the notice's box, whole");
            }
            assert!(
                (0..notice.len())
                    .flat_map(|col| crop_cell(&arms, &frame, (row, col)))
                    .any(|p| p == Pair::BAUD.text.0),
                "{name}: the notice's own colour was drawn"
            );
        }
    }
}

/// **No cursor covers an address, on any arm**: a visible block cursor moved
/// onto a letter of the bar, in `#F4A32C`'s own colour by `OSC 12` or any
/// other, draws nothing over it, so a program cannot hide a letter of where a
/// link goes. The control: the same cursor with no bar reaches the pixels.
/// Perturbations: drop `clear_of_messages` from `convert_with` (the CPU and
/// instance arms fail) or from the harness's and the app's cell path (the cell
/// arm disagrees); read it for the tip's lines alone.
#[test]
fn no_cursor_covers_an_address_on_any_arm() {
    use baud_render::Pair;
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let built = read("baud-app/fonts/built/Regular.ttf");
    let faces = [Face::parse(&built).expect("parse")];
    let (cols, rows) = (
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    );
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            cols,
            rows,
        )
        .unwrap()
    };
    let look = address_look("https://example.org/docs");
    let last = usize::from(WS.ws_row) - 1;
    let put = format!("\x1b[{};4H", last + 1);
    let programs: [(&str, &[u8]); 4] = [
        ("a block", b"\x1b[2 q"),
        ("a bar", b"\x1b[6 q"),
        ("an underline", b"\x1b[4 q"),
        (
            "a block in the box's colour",
            b"\x1b[2 q\x1b]12;#F4A32C\x1b\\",
        ),
    ];
    for (name, bytes) in programs {
        arms!(fresh() => arms, t);
        feed(&mut t, b"some text under it");
        feed(&mut t, ENOUGH_GLYPHS);
        feed(&mut t, bytes);
        feed(&mut t, put.as_bytes());
        arms.renderer.set_cursor_blink(Blink::Shown);
        let bare = three_arms(&mut arms, &mut t, name);
        arms.link = Some(look.clone());
        let hidden = three_arms(&mut arms, &mut t, name);
        arms.link = None;
        feed(&mut t, b"\x1b[?25l");
        arms.link = Some(look.clone());
        let none = three_arms(&mut arms, &mut t, name);
        arms.link = None;
        assert_eq!(
            differing(&hidden, &none),
            0,
            "{name}: the bar with the cursor on it is the bar with no cursor"
        );
        for p in crop_cell(&arms, &hidden, (last, 0)) {
            assert_eq!(p, Pair::BAUD.page.0, "{name}: the box, whole");
        }
        feed(&mut t, b"\x1b[?25h");
        let plain = three_arms(&mut arms, &mut t, name);
        assert_eq!(differing(&bare, &plain), 0, "the control's own frame again");
        feed(&mut t, b"\x1b[?25l");
        let hid = three_arms(&mut arms, &mut t, name);
        assert!(
            differing(&plain, &hid) > 0,
            "{name}, the control: with no bar the cursor reaches the pixels"
        );
    }
    let source = include_str!("../../baud-app/src/app.rs");
    assert!(
        source.contains(concat!(
            "self.renderer.clear_of_",
            "messages(&self.term, cursor_at)"
        )),
        "the app's cell path takes its cursor through the rule this harness copies"
    );
}

/// **The lines at rest and all three looks under the pointer are one picture
/// on every arm**: a line in the text's colour on two rows, with and without a
/// program's colours, solid and dotted; the box, the line and the text.
/// Perturbations: leave `line_inst` out of `compose_inst` (the instance and CPU
/// arms lose it), or out of the cell path's records (the cell arm does); give
/// the text look a page; drop a look's fields from the cell path's chrome.
#[test]
fn the_lines_and_the_looks_are_one_picture_on_every_arm() {
    use baud_render::{HoverPaint, LineColour, LineStyle, LinkHover, LinkLine, LinkSpan};
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let built = read("baud-app/fonts/built/Regular.ttf");
    let faces = [Face::parse(&built).expect("parse")];
    let (cols, rows) = (
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    );
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            cols,
            rows,
        )
        .unwrap()
    };
    arms!(fresh() => arms, t);
    feed(
        &mut t,
        b"\x1b[?25lsee \x1b[31mhttps://example.org/x\x1b[m and \x1b[44;33mhttps://b.example\x1b[m",
    );
    feed(&mut t, ENOUGH_GLYPHS);
    let bare = three_arms(&mut arms, &mut t, "no lines");
    let word = |x: usize| {
        let c = t.view(Rows::ZERO).row(Row::ZERO)[x];
        t.style_in(baud_domain::RowStyles::Live, c)
    };
    arms.lines = vec![
        LinkLine {
            row: Row::ZERO,
            from: Col::at_index(4),
            to: Col::at_index(24),
            word: word(4),
        },
        LinkLine {
            row: Row::ZERO,
            from: Col::at_index(30),
            to: Col::at_index(46),
            word: word(30),
        },
    ];
    let solid = three_arms(&mut arms, &mut t, "solid");
    assert!(differing(&bare, &solid) > 0, "the line reached the pixels");
    arms.line_style = LineStyle::Dotted;
    let dotted = three_arms(&mut arms, &mut t, "dotted");
    assert!(differing(&solid, &dotted) > 0, "dotted is not solid");
    arms.line_style = LineStyle::Solid;
    for paint in [HoverPaint::Box, HoverPaint::Line, HoverPaint::Text] {
        // The lines at rest as the driver sets them under this look
        // (`config::Link::line_colour`).
        arms.renderer.set_line_colour(match paint {
            HoverPaint::Line => LineColour::Text,
            HoverPaint::Box | HoverPaint::Text => LineColour::Box,
        });
        let rest = three_arms(&mut arms, &mut t, "at rest");
        arms.over = Some(LinkHover {
            spans: vec![LinkSpan {
                row: Row::ZERO,
                from: Col::at_index(4),
                to: Col::at_index(24),
            }],
            paint,
        });
        let over = three_arms(&mut arms, &mut t, "a look");
        assert!(differing(&rest, &over) > 0, "{paint:?} reached the pixels");
        arms.over = None;
        let gone = three_arms(&mut arms, &mut t, "gone");
        assert_eq!(differing(&rest, &gone), 0, "{paint:?} leaves nothing");
    }
    arms.renderer.set_line_colour(LineColour::Box);
}

/// **An unhovered link's line is the box's colour on every arm, and a
/// hovered link looks as it did**: the instance, cell and CPU arms draw every
/// pixel of the line under two links of two text colours in `[message]`'s
/// default box colour, `Pair::BAUD`'s `#F4A32C`; with the pointer over the
/// first, `"text"` keeps its line in the text's own red and `"box"` covers
/// it, as before, while the second stays amber; under `"line"` the lines at
/// rest keep the text's colour, as before, and the hovered one is the box's.
/// The control: the same lines under no look are not the text's colour.
/// Perturbations: draw the line at rest in the text's colour (`rest` in
/// `Renderer::place_links_with`); draw the hovered link's line amber too,
/// which `"text"` shows; ignore `LineColour::Text`, which `"line"` shows.
#[test]
fn an_unhovered_line_is_amber_and_a_hovered_one_unchanged_on_every_arm() {
    use baud_render::{HoverPaint, LineColour, LineStyle, LinkHover, LinkLine, LinkSpan, Pair};
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let built = read("baud-app/fonts/built/Regular.ttf");
    let faces = [Face::parse(&built).expect("parse")];
    let (cols, rows) = (
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    );
    let renderer = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        cols,
        rows,
    )
    .unwrap();
    arms!(renderer => arms, t);
    feed(
        &mut t,
        b"\x1b[?25lsee \x1b[38;2;255;0;0mhttps://example.org/x\x1b[m and \
          \x1b[38;2;0;255;0;44mhttps://b.example\x1b[m",
    );
    feed(&mut t, ENOUGH_GLYPHS);
    let plain = three_arms(&mut arms, &mut t, "no lines");
    let word = |x: usize| {
        let c = t.view(Rows::ZERO).row(Row::ZERO)[x];
        t.style_in(baud_domain::RowStyles::Live, c)
    };
    let (first, second) = ((4, 24), (30, 46));
    arms.lines = [first, second]
        .map(|(from, to)| LinkLine {
            row: Row::ZERO,
            from: Col::at_index(from),
            to: Col::at_index(to),
            word: word(from),
        })
        .to_vec();
    arms.line_style = LineStyle::Solid;
    let m = arms.renderer.metrics();
    let w = usize::try_from(arms.renderer.whole_size().0.value()).unwrap();
    let cw = usize::try_from(m.cell_w.value()).unwrap();
    let y = usize::try_from(m.underline.value()).unwrap();
    // The line's row of pixels over a span, where no glyph's ink reaches it
    // in the frame with no lines: the pixels its cells' page fills there.
    let rgb = |f: &[u32], (from, to): (usize, usize)| -> Vec<u32> {
        let xs = from * cw..(to + 1) * cw;
        let page = |x: usize| plain[y * w + x];
        let mut seen: Vec<(u32, usize)> = Vec::new();
        for x in xs.clone() {
            match seen.iter_mut().find(|(p, _)| *p == page(x)) {
                Some((_, n)) => *n += 1,
                None => seen.push((page(x), 1)),
            }
        }
        let most = seen.iter().max_by_key(|(_, n)| *n).map(|(p, _)| *p);
        let clear: Vec<usize> = xs.filter(|&x| Some(page(x)) == most).collect();
        assert!(
            clear.len() * 2 > (to + 1 - from) * cw,
            "most of the row is clear"
        );
        clear.iter().map(|&x| f[y * w + x] & 0x00FF_FFFF).collect()
    };
    let all = |v: &[u32], c: u32| v.iter().all(|&p| p == c);
    let amber = Pair::BAUD.page.0 & 0x00FF_FFFF;
    let rest = three_arms(&mut arms, &mut t, "at rest");
    for (span, what) in [(first, "red text"), (second, "green text")] {
        assert!(
            all(&rgb(&rest, span), amber),
            "{what}: the line at rest is #F4A32C on every arm, found {:06x?}",
            &rgb(&rest, span)[..4]
        );
    }
    assert!(!all(&rgb(&rest, first), 0xFF_0000), "the control");
    let hovered = |paint| LinkHover {
        spans: vec![LinkSpan {
            row: Row::ZERO,
            from: Col::at_index(first.0),
            to: Col::at_index(first.1),
        }],
        paint,
    };
    arms.over = Some(hovered(HoverPaint::Text));
    let text = three_arms(&mut arms, &mut t, "text");
    assert!(
        all(&rgb(&text, first), 0xFF_0000),
        "\"text\": the hovered line is the text's red, found {:06x?}",
        &rgb(&text, first)[..4]
    );
    assert!(
        all(&rgb(&text, second), amber),
        "\"text\": the other at rest"
    );
    arms.renderer.set_line_colour(LineColour::Text);
    arms.over = Some(hovered(HoverPaint::Line));
    let line = three_arms(&mut arms, &mut t, "line");
    assert!(all(&rgb(&line, first), amber), "\"line\": the box's colour");
    assert!(
        all(&rgb(&line, second), 0x00_FF00),
        "\"line\": the other at rest in its text's green"
    );
    arms.renderer.set_line_colour(LineColour::Box);
    arms.over = Some(hovered(HoverPaint::Box));
    let boxed = three_arms(&mut arms, &mut t, "box");
    let kept = arms.lines.clone();
    arms.lines = vec![kept[1]];
    let bare = three_arms(&mut arms, &mut t, "box without the first line");
    assert_eq!(
        rgb(&boxed, first),
        rgb(&bare, first),
        "\"box\": the box covers the line under it"
    );
    assert!(
        all(&rgb(&boxed, second), amber),
        "\"box\": the other at rest"
    );
    arms.lines = kept;
    arms.over = None;
}
/// The alaph of `word`, written from column 3 of row 2 after a clear, its
/// cell's pixels on every arm: a letter, an erased cell and the alaph,
/// whose logical column is 4 from zero.
fn alaph_cell(arms: &mut Arms<'_, '_>, t: &mut Term<'_>, word: &str) -> Vec<u32> {
    feed(t, b"\x1b[?25l\x1b[0m\x1b[0 ]\x1b[2J\x1b[H\x1b[8h");
    feed(
        t,
        "\x1b[8;3HThe quick brown fox jumps 0123456789".as_bytes(),
    );
    feed(t, format!("\x1b[2;3H{word}").as_bytes());
    let frame = three_arms(arms, t, word);
    crop_cell(arms, &frame, (1, 4))
}

/// **Where SAPV meets a letter's word, on every arm**
/// (`[[source: ecma48]]` 8.3.108): on Noto Sans Syriac an alaph's `rlig`
/// picks a wider final after a beth than after a yudh, and an erased cell
/// between the two letters, which joining passes through and the word
/// leaves out, keeps each letter's ink out of the other's cell. Under 17 a
/// beth and an alaph both italic are drawn upright and sheared left, and
/// the alaph reads its beth as it does when only the alaph is italic: the
/// beth, a slanted cell, is drawn by its upright face, the alaph's, though
/// the stack's italic face draws Syriac too. Under 8 an alaph inside a
/// word, med2 by joining, is drawn final, reading the beth before it as a
/// final alaph does.
#[test]
fn sapv_meets_a_letters_word_on_every_arm() {
    use baud_render::{FaceRole, FaceStyle};
    let built = std::fs::read(format!(
        "{}/../baud-app/fonts/built/Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("face");
    let syriac = std::fs::read("/System/Library/Fonts/Supplemental/NotoSansSyriac-Regular.ttf")
        .expect("Noto Sans Syriac, which macOS ships; this test cannot skip itself");
    // The italic face a second Noto Sans Syriac: a face of its own for
    // the italic style, which draws Syriac.
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&syriac).expect("parse"),
        Face::parse(&syriac).expect("parse"),
    ];
    let roles = [
        FaceRole::Fallback,
        FaceRole::Fallback,
        FaceRole::Style(FaceStyle::Italic),
    ];
    let renderer = Renderer::new(
        Stack::with_roles(&faces, &roles).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    arms!(renderer => arms, t);
    let (beth, yudh, alaph) = ('\u{0712}', '\u{071D}', '\u{0710}');
    let after_beth = alaph_cell(&mut arms, &mut t, &format!("{beth}\x1b[C{alaph}"));
    let after_yudh = alaph_cell(&mut arms, &mut t, &format!("{yudh}\x1b[C{alaph}"));
    assert_ne!(
        after_beth, after_yudh,
        "the premise: the rlig reads the beth"
    );
    let slant = "\x1b[3m\x1b[17 ]";
    let alone = alaph_cell(&mut arms, &mut t, &format!("{beth}\x1b[C{slant}{alaph}"));
    let both = alaph_cell(&mut arms, &mut t, &format!("{slant}{beth}\x1b[C{alaph}"));
    let both_yudh = alaph_cell(&mut arms, &mut t, &format!("{slant}{yudh}\x1b[C{alaph}"));
    assert_ne!(alone, after_beth, "the premise: 17 slants the alaph");
    assert_ne!(both, both_yudh, "17: the slanted alaph reads its beth");
    assert_eq!(
        both, alone,
        "17: the slanted beth is read on its upright face"
    );
    let asked = alaph_cell(
        &mut arms,
        &mut t,
        &format!("{beth}\x1b[C\x1b[8 ]{alaph}\x1b[C{beth}"),
    );
    assert_eq!(asked, after_beth, "8: final, with its word");
    assert_ne!(asked, after_yudh, "8: not the final drawn alone");
}

/// **Digits SAPV draws as Arabic-Indic are read in a letter's word as
/// drawn, on every arm** (`[[source: ecma48]]` 8.3.108, values 2 and 10):
/// with BDSM reset a row is one run, so a joined letter's word holds the
/// digits and the decimal mark beside it, read as the Arabic-Indic digits
/// and the comma they are drawn as, and the row draws as those characters
/// written, on Courier New, which draws both. A frame after the digits'
/// SAPV changes, the letters' cells unchanged, draws as a fresh renderer
/// draws it.
#[test]
fn a_letters_word_reads_its_digits_as_drawn_on_every_arm() {
    let built = std::fs::read(format!(
        "{}/../baud-app/fonts/built/Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("face");
    let courier = std::fs::read("/System/Library/Fonts/Supplemental/Courier New.ttf")
        .expect("Courier New, which macOS ships; this test cannot skip itself");
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&courier).expect("parse"),
    ];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let beh = '\u{0628}';
    let scene = |digits: &str| {
        format!(
            "\x1bc\x1b[?25l\x1b[8l\x1b[8;3HThe quick brown fox jumps 0123456789\
             \x1b[2;3H{beh}{beh}{digits}\x1b[0 ]"
        )
    };
    let varied = scene("\x1b[2;10 ]3.1");
    let written = scene("\u{0663},\u{0661}");
    arms!(fresh() => arms, t);
    feed(&mut t, varied.as_bytes());
    let got = three_arms(&mut arms, &mut t, "varied");
    feed(&mut t, written.as_bytes());
    let want = three_arms(&mut arms, &mut t, "written");
    assert_eq!(differing(&got, &want), 0, "drawn as the characters drawn");
    // The digits' SAPV changes under unchanged letters.
    arms!(fresh() => arms, t);
    feed(&mut t, scene("3.1").as_bytes());
    let stored = three_arms(&mut arms, &mut t, "stored");
    feed(&mut t, varied.as_bytes());
    let again = three_arms(&mut arms, &mut t, "varied after stored");
    assert_ne!(differing(&stored, &again), 0, "the premise: 2 and 10 draw");
    arms!(fresh() => fresh_arms, u);
    feed(&mut u, varied.as_bytes());
    let fresh_frame = three_arms(&mut fresh_arms, &mut u, "varied, fresh");
    assert_eq!(
        differing(&again, &fresh_frame),
        0,
        "as a fresh renderer draws it"
    );
}

/// **A lam-alef pair SAPV 17 slants is read in its word as it is
/// upright, on every arm** (`[[source: ecma48]]` 8.3.108): on SF Arabic,
/// macOS's own Arabic face, an initial kaf before a lam draws the form
/// its `rclt` picks there and not the one before a meem, so a kaf before
/// a lam-alef pair reads the pair's lam in its word. Italic under 17 on a
/// right-to-left row, the pair is drawn as its upright glyph sheared
/// left; the kaf before it, upright, reads the slanted lam on its upright
/// face, as it reads it upright, though the stack's italic face draws
/// Arabic too; a slanted kaf reads its word; and the pair draws alike
/// whether the kaf is slanted or not. With BDSM set, an erased cell
/// between the kaf and the pair, which joining passes through and the
/// word leaves out, keeping each one's ink out of the other's cells; with
/// BDSM reset on a right-to-left path, where an erased cell parts two
/// letters, the kaf touching the pair, whose cells a slanted kaf's ink
/// then reaches, so the pair is not compared there.
#[test]
fn a_slanted_lam_alef_is_read_in_its_word_on_every_arm() {
    use baud_render::{FaceRole, FaceStyle};
    let built = std::fs::read(format!(
        "{}/../baud-app/fonts/built/Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("face");
    let arabic = std::fs::read("/System/Library/Fonts/SFArabic.ttf")
        .expect("SF Arabic, which macOS ships; this test cannot skip itself");
    // The italic face a second SF Arabic: a face of its own for the
    // italic style, which draws Arabic.
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&arabic).expect("parse"),
        Face::parse(&arabic).expect("parse"),
    ];
    let roles = [
        FaceRole::Fallback,
        FaceRole::Fallback,
        FaceRole::Style(FaceStyle::Italic),
    ];
    let renderer = Renderer::new(
        Stack::with_roles(&faces, &roles).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    arms!(renderer => arms, t);
    let (kaf, lam, meem, alef) = ('\u{0643}', '\u{0644}', '\u{0645}', '\u{0627}');
    let slant = "\x1b[3m\x1b[17 ]";
    for (mode, gap) in [("\x1b[8h", "\x1b[C"), ("\x1b[8l\x1b[2 k", "")] {
        // The kaf's cell and the two after it, written from column 3 of
        // row 2 with `gap` between the kaf and them.
        let at = 3 + gap.len().min(1);
        let cells = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, word: &str| {
            feed(t, b"\x1b[?25l\x1b[0m\x1b[0 ]\x1b[2J\x1b[H");
            feed(
                t,
                format!("\x1b[8;3HThe quick brown fox jumps 0123456789\x1b[2;1H{mode}").as_bytes(),
            );
            feed(t, format!("\x1b[2;3H{word}").as_bytes());
            let frame = three_arms(arms, t, &format!("{mode:?} {word:?}"));
            let mut pair = crop_cell(arms, &frame, (1, at));
            pair.extend(crop_cell(arms, &frame, (1, at + 1)));
            (crop_cell(arms, &frame, (1, 2)), pair)
        };
        let (upright, upright_pair) = cells(&mut arms, &mut t, &format!("{kaf}{gap}{lam}{alef}"));
        let r = Row::from_index(1);
        let v = arms.renderer.bidi().visual_col(r, Col::at_index(at));
        assert!(
            arms.renderer
                .bidi()
                .joined(r, v)
                .is_some_and(|j| j.pair.is_some()),
            "{mode:?}: the premise, the lam and alef make a pair"
        );
        let (before_meem, _) = cells(&mut arms, &mut t, &format!("{kaf}{gap}{meem}{alef}"));
        let (alone, alone_pair) =
            cells(&mut arms, &mut t, &format!("{kaf}{gap}{slant}{lam}{alef}"));
        let (both, both_pair) = cells(&mut arms, &mut t, &format!("{slant}{kaf}{gap}{lam}{alef}"));
        let (both_meem, _) = cells(&mut arms, &mut t, &format!("{slant}{kaf}{gap}{meem}{alef}"));
        assert_ne!(
            upright, before_meem,
            "{mode:?}: the premise, the kaf's rclt reads the lam after it"
        );
        assert_eq!(
            alone, upright,
            "{mode:?}: the kaf reads the slanted lam on its upright face"
        );
        assert_ne!(alone_pair, upright_pair, "{mode:?}: 17 slants the pair");
        assert_ne!(both, both_meem, "{mode:?}: the slanted kaf reads its word");
        // Touching, a slanted kaf's ink reaches the pair's cells.
        if !gap.is_empty() {
            assert_eq!(
                both_pair, alone_pair,
                "{mode:?}: the pair draws alike beside a slanted kaf"
            );
        }
    }
}

/// **`text` drawn on every arm as the first frame of a renderer of its
/// own**: the cell arm walks the screen before any conversion has
/// resolved the renderer's own rendition table, as the app's cell path,
/// which converts nothing, walks every frame. The arms share one
/// renderer, so a scene drawn after another reads what the last frame's
/// conversion resolved there, and a rendition first named this frame is
/// missing from it. Every arm draws the instance arm's pixels, and every
/// glyph the cell arm's records draw is one of the instance arm's -- the
/// same atlas slot at the same place, so the same face's glyph -- which
/// tells two faces that draw alike apart. The instance arm's frame, the
/// cells the records drew, and the cell `crop` of the frame.
fn first_frame(
    fonts: (&[Face<'_>], &[FaceRole]),
    text: &str,
    crop: (usize, usize),
    what: &str,
) -> (Vec<u32>, Tally, Vec<u32>) {
    first_frame_with(fonts, |_| {}, text, crop, what)
}

/// [`first_frame`] with `setup` done to the terminal before `text` is
/// fed: an embedder's setting, which no byte sets.
fn first_frame_with(
    (faces, roles): (&[Face<'_>], &[FaceRole]),
    setup: impl FnOnce(&mut Term<'_>),
    text: &str,
    crop: (usize, usize),
    what: &str,
) -> (Vec<u32>, Tally, Vec<u32>) {
    let renderer = Renderer::new(
        Stack::with_roles(faces, roles).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    arms!(renderer => arms, t);
    arms.reads = Reads::Copy;
    setup(&mut t);
    feed(&mut t, text.as_bytes());
    let (a, b) = arms.draw(&mut t);
    let glyph = |i: &Instance| (i.kind, i.x, i.y, i.w, i.h, i.u, i.v);
    let instances: Vec<_> = arms.renderer.instances().map(glyph).collect();
    let stray = arms
        .records
        .iter()
        .filter(|i| i.kind == Kind::Mask && !instances.contains(&glyph(i)))
        .count();
    let copied = arms.draw_copied(&mut t);
    let cpu = cpu_frame(&mut arms, &mut t);
    assert_eq!(differing(&a, &b), 0, "{what}: the cell arm differs");
    assert_eq!(
        differing(&a, &copied),
        0,
        "{what}: the cell arm over the copy differs"
    );
    assert_eq!(differing(&a, &cpu), 0, "{what}: the CPU arm differs");
    assert_eq!(
        stray, 0,
        "{what}: glyph records the instance arm does not draw"
    );
    let cell = crop_cell(&arms, &a, crop);
    (a, arms.exceptions.stats().cells, cell)
}

/// **An italic letter reads its word on every arm** where the stack's
/// italic face draws its script: on Noto Sans Syriac an alaph after a
/// beth, with an erased cell between them and touching, takes the wider
/// final its `rlig` picks there, and on SF Arabic a kaf before a lam-alef
/// pair the form its `rclt` picks before a lam, with BDSM set and reset --
/// all in `SGR 3` and no SAPV, each the first frame of a renderer of its
/// own (`first_frame`); and an alaph composed after an italic beth and an
/// erased cell, over an italic blank, draws as the same alaph written
/// there. While a joined letter read its neighbours' styles from the
/// renderer's own rendition table, which the cell path does not resolve
/// and a composition set aside while it drew, the cell arm read the italic
/// beth and lam as regular, on another face than the letter's, and every
/// arm drew the composed alaph so, each letter as if alone. The premises:
/// the italic alaph after a beth is not the one after a yudh, the italic
/// kaf before a lam not the one before a meem, and the composition joins.
#[test]
fn an_italic_letter_reads_its_word_on_every_arm() {
    let built = std::fs::read(format!(
        "{}/../baud-app/fonts/built/Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("face");
    let syriac = std::fs::read("/System/Library/Fonts/Supplemental/NotoSansSyriac-Regular.ttf")
        .expect("Noto Sans Syriac, which macOS ships; this test cannot skip itself");
    let arabic = std::fs::read("/System/Library/Fonts/SFArabic.ttf")
        .expect("SF Arabic, which macOS ships; this test cannot skip itself");
    // The italic face a second copy of the script's face: a face of its
    // own for the italic style, which draws the script.
    let roles = [
        FaceRole::Fallback,
        FaceRole::Fallback,
        FaceRole::Style(FaceStyle::Italic),
    ];
    let screen = |mode: &str, word: &str| {
        format!(
            "\x1b[?25l\x1b[8;3HThe quick brown fox jumps 0123456789\
             \x1b[2;1H{mode}\x1b[2;3H\x1b[3m{word}"
        )
    };
    let syriac_faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&syriac).expect("parse"),
        Face::parse(&syriac).expect("parse"),
    ];
    let (beth, yudh, alaph) = ('\u{0712}', '\u{071D}', '\u{0710}');
    let mut alaphs = Vec::new();
    for (word, at) in [
        (format!("{beth}\x1b[C{alaph}"), 4),
        (format!("{yudh}\x1b[C{alaph}"), 4),
        (format!("{beth}{alaph}"), 3),
    ] {
        let what = format!("Syriac {word:?}");
        let (_, _, cell) = first_frame(
            (&syriac_faces, &roles),
            &screen("\x1b[8h", &word),
            (1, at),
            &what,
        );
        alaphs.push(cell);
    }
    assert_ne!(
        alaphs[0], alaphs[1],
        "the premise: the italic alaph reads its beth"
    );
    // **Composed**: an alaph composed after an italic beth and an erased
    // cell, over an italic blank, draws as the same alaph written there.
    let renderer = Renderer::new(
        Stack::with_roles(&syriac_faces, &roles).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    arms!(renderer => arms, t);
    let failed = std::cell::RefCell::new(Vec::<String>::new());
    let fail = |what: String| failed.borrow_mut().push(what);
    let scene = format!(
        "\x1bc\x1b[2 q\x1b[8h\x1b[2;3HThe quick brown fox jumps 0123456789\
         \x1b[3;3H\x1b[3m{beth}\x1b[C \x1b[D"
    );
    let joined = composed_as_written(
        &mut arms,
        &mut t,
        (&scene, &alaph.to_string()),
        "Syriac composed",
        &three_arms,
        &fail,
    );
    assert!(joined, "the premise: the composition joins");
    let failed = failed.into_inner();
    assert!(failed.is_empty(), "{failed:#?}");
    let arabic_faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&arabic).expect("parse"),
        Face::parse(&arabic).expect("parse"),
    ];
    let (kaf, lam, meem, alef) = ('\u{0643}', '\u{0644}', '\u{0645}', '\u{0627}');
    for mode in ["\x1b[8h", "\x1b[8l\x1b[2 k"] {
        let mut kafs = Vec::new();
        for word in [
            format!("{kaf}{lam}{alef}"),
            format!("{kaf}{meem}{alef}"),
            format!("{kaf}\x1b[C{lam}{alef}"),
        ] {
            let what = format!("Arabic {mode:?} {word:?}");
            let (_, _, cell) =
                first_frame((&arabic_faces, &roles), &screen(mode, &word), (1, 2), &what);
            kafs.push(cell);
        }
        assert_ne!(
            kafs[0], kafs[1],
            "{mode:?}: the premise, the italic kaf reads the lam after it"
        );
    }
}

/// **A status line's joined letter reads its word, on every arm**, as a
/// main row's does: an alaph after a beth on the host-writable status line
/// draws the wider final Noto Sans Syriac's `rlig` picks after a beth, the
/// pixels of the same alaph on a row of the main display, and not the one
/// after a yudh. Each status line is written from its first column, and
/// each alaph's cell is checked to hold ink before the two are compared.
#[test]
fn a_status_lines_letter_reads_its_word_on_every_arm() {
    let built = std::fs::read(format!(
        "{}/../baud-app/fonts/built/Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("face");
    let syriac = std::fs::read("/System/Library/Fonts/Supplemental/NotoSansSyriac-Regular.ttf")
        .expect("Noto Sans Syriac, which macOS ships; this test cannot skip itself");
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&syriac).expect("parse"),
    ];
    let renderer = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    arms!(renderer => arms, t);
    let (beth, yudh, alaph) = ('\u{0712}', '\u{071D}', '\u{0710}');
    let main = alaph_cell(&mut arms, &mut t, &format!("{beth}\x1b[C{alaph}"));
    let status = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, first: char| {
        feed(t, b"\x1b[?25l\x1b[2J\x1b[H\x1b[8h");
        feed(
            t,
            format!("\x1b[2$~\x1b[1$}}\x1b[2K\x1b[1G\u{5D0} {first}\x1b[C{alaph}\x1b[0$}}")
                .as_bytes(),
        );
        arms.renderer.fit(t);
        let frame = three_arms(arms, t, "the status line");
        let row = t.view(Rows::ZERO).rows().index();
        let cell = crop_cell(arms, &frame, (row, 4));
        assert!(
            cell.iter().any(|&p| p != cell[0]),
            "the premise: the alaph after {first:?} is drawn in the cell compared"
        );
        cell
    };
    let on_status = status(&mut arms, &mut t, beth);
    let on_status_yudh = status(&mut arms, &mut t, yudh);
    assert_eq!(
        status(&mut arms, &mut t, beth),
        on_status,
        "the premise: a second status line written over the first draws as it"
    );
    assert_ne!(on_status, on_status_yudh, "the status line's rlig reads");
    assert_eq!(on_status, main, "the status line's alaph as a main row's");
}

/// **The cell path with BDSM reset finds a row's joins again only when the
/// copy's compare finds the row changed** (`Screens::take`, research
/// decision D4): once the renderer has shaped a letter that takes a joining
/// form, a frame whose screen is the last's reads no row for its joins, a
/// frame that changed one row tests that row alone, and the joins it keeps
/// are always the ones a fresh layout finds. The frame is the app's own
/// sequence, through the functions the app calls.
#[test]
fn the_cell_path_rejoins_only_the_rows_the_copy_finds_changed() {
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let built = read("baud-app/fonts/built/Regular.ttf");
    let faces = [Face::parse(&built).expect("parse")];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let rows = u64::from(WS.ws_row);
    let mut screens = Screens::new();
    let mut copy = vec![PackedCell::EMPTY; t.stride().index() * (usize::from(WS.ws_row) + 2)];
    let (mut cells, mut used) = (Vec::new(), Vec::new());
    // One frame of the cell path as the app builds it, to the walk: what
    // the layout read, and whether its joins are a fresh layout's.
    let mut frame = |r: &mut Renderer<'_, '_>, t: &Term<'_>| {
        let rejoin = r.lay_out_bidi_copied(t, Rows::ZERO);
        let view = r.bidi().window(t, Rows::ZERO, &mut cells, &mut used);
        screens.compare = Compare::for_rejoin(rejoin);
        screens.take(
            &mut [&mut copy[..]],
            SetIx(0),
            &view,
            t.status_line().as_ref(),
            t.stride(),
            baud_render::Leaving::None,
        );
        if let Some(how) = rejoin {
            r.rejoin_bidi(t, Rows::ZERO, how, |row| screens.changed(row));
        }
        let mut fresh = baud_render::Reorder::new();
        fresh.set_reach(r.joined_reach());
        fresh.lay_out(t, Rows::ZERO);
        for y in 0..usize::from(WS.ws_row) {
            let row = Row::from_index(y);
            assert_eq!(
                r.bidi().row_joins(row),
                fresh.orders().row_joins(row),
                "row {y}: the kept joins are not the screen's"
            );
        }
        let w = r.bidi_work();
        (w.compared.value(), w.tested.value())
    };
    feed(
        &mut t,
        "\x1b[?25l\x1b[8l\x1b[3;1H\u{0628}\u{0628}".as_bytes(),
    );
    r.convert(&t, Rows::ZERO, Damage::Full);
    assert_eq!(r.joins_seen(), baud_render::Joins::Seen, "a beh was shaped");
    assert_eq!(
        frame(&mut r, &t),
        (0, rows),
        "after a conversion: every row"
    );
    assert_eq!(frame(&mut r, &t), (0, 0), "the same screen");
    assert_eq!(frame(&mut r, &t), (0, 0), "and again");
    feed(&mut t, b"\x1b[1;1Hj");
    assert_eq!(frame(&mut r, &t), (0, 1), "one Latin row changed");
    feed(&mut t, b"\x1b[3;1Hxx");
    assert_eq!(frame(&mut r, &t), (0, 1), "the Arabic row made Latin");
    feed(&mut t, "\x1b[5;1H\u{0628}\u{0628}\u{0628}".as_bytes());
    assert_eq!(frame(&mut r, &t), (0, 1), "a Latin row made Arabic");
    assert!(
        r.bidi().joins(Row::from_index(4)).is_some(),
        "its behs joined"
    );
    assert_eq!(frame(&mut r, &t), (0, 0), "the same screen, a row joined");
    r.convert(&t, Rows::ZERO, Damage::Incremental);
    assert_eq!(frame(&mut r, &t), (0, rows), "after a conversion again");
    assert_eq!(frame(&mut r, &t), (0, 0), "and the same screen");
}

/// **A mirrored character resolved right to left is drawn mirrored on every
/// arm** (UAX #9 L4, `[[source: uax9-bidi]]`): U+2211, one of the 126
/// Bidi_Mirrored characters with no Bidi_Mirroring_Glyph, which
/// BidiMirroring.txt leaves "up to the rendering system to provide
/// mirrored glyphs" for (`[[source: unicode-bidimirroring]]`), draws its
/// own glyph flipped left to right in its cell, as U+221A does; U+2203
/// carrying U+0338,
/// the decomposed U+2204 which has none either, draws the glyph the face
/// composes flipped; and `<` carrying U+0338, whose base has a mirroring
/// glyph, draws `>` under the mark, as `>` and U+0338 written in its place
/// draw. Each is after an alef, at the end of a right-to-left paragraph,
/// so N2 gives it R.
///
/// The expected frame is the same rows written in their visual order with
/// BDSM reset, the flipped cells mirrored in the frame by hand, pixel for
/// pixel: the instance arm, the cell arm and the CPU arm must each be it.
#[test]
fn mirrored_characters_draw_mirrored_on_every_arm() {
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let built = read("baud-app/fonts/built/Regular.ttf");
    let faces = [Face::parse(&built).expect("parse")];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let cols = usize::from(WS.ws_col);
    let alef = "\u{5D0}";
    // Each row as written with BDSM set, from column 1, and as drawn,
    // written with it reset two cells from the right edge; and whether its
    // first cell there is drawn flipped.
    let rows: [(String, String, bool); 4] = [
        (format!("{alef}\u{2211}"), format!("\u{2211}{alef}"), true),
        (format!("{alef}\u{221A}"), format!("\u{221A}{alef}"), true),
        (
            format!("{alef}\u{2203}\u{338}"),
            format!("\u{2203}\u{338}{alef}"),
            true,
        ),
        (format!("{alef}<\u{338}"), format!(">\u{338}{alef}"), false),
    ];
    let scene = |t: &mut Term<'_>, implicit: bool| {
        feed(t, b"\x1b[?25l\x1b[2J\x1b[H");
        feed(t, if implicit { b"\x1b[8h" } else { b"\x1b[8l" });
        for (i, (logical, visual, _)) in rows.iter().enumerate() {
            let (text, col) = match implicit {
                true => (logical, 1),
                false => (visual, cols - 1),
            };
            feed(t, format!("\x1b[{};{col}H{text}", 2 * i + 2).as_bytes());
        }
        feed(t, b"\x1b[10;3HThe quick brown fox jumps 0123456789");
    };
    let three = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, what: &str| {
        arms.reads = Reads::Copy;
        let (a, _) = arms.draw(t);
        let b = arms.draw_copied(t);
        let c = cpu_frame(arms, t);
        assert_eq!(differing(&a, &b), 0, "{what}: the cell arm differs");
        assert_eq!(differing(&a, &c), 0, "{what}: the CPU arm differs");
        a
    };
    arms!(fresh() => arms, t);
    scene(&mut t, true);
    let got = three(&mut arms, &mut t, "mirrored");
    assert!(arms.renderer.bidi().any(), "the rows were reordered");
    scene(&mut t, false);
    let mut want = three(&mut arms, &mut t, "written in visual order");
    let w = usize::try_from(arms.renderer.whole_size().0.value()).unwrap();
    let cw = usize::try_from(arms.renderer.metrics().cell_w.value()).unwrap();
    let ch = usize::try_from(arms.renderer.metrics().cell_h.value()).unwrap();
    let col = cols - 2;
    for (i, (_, _, flipped)) in rows.iter().enumerate() {
        if !flipped {
            continue;
        }
        let r = 2 * i + 1;
        for y in r * ch..(r + 1) * ch {
            let at = y * w + col * cw;
            want[at..at + cw].reverse();
        }
    }
    // The control: each flipped cell is not its own mirror, so its row
    // tests the flip.
    let plain = three(&mut arms, &mut t, "unmirrored");
    for (i, (logical, _, flipped)) in rows.iter().enumerate() {
        let band = (2 * i + 1) * ch * w..(2 * i + 2) * ch * w;
        assert!(
            !flipped || differing(&want[band.clone()], &plain[band]) > 0,
            "the control: {logical:?}'s glyph is not its own mirror"
        );
    }
    assert_eq!(
        diff_box(&got, &want, w, (cw, ch)),
        None,
        "each mirrored character drawn mirrored"
    );
    // **A mark on a flipped base is not flipped** (L4 mirrors a character
    // "if and only if" it is Bidi_Mirrored, which U+0301 is not): U+2211
    // carrying an acute, which the face draws as two glyphs, is drawn
    // alike on every arm, its base mirrored and its acute neither left as
    // drawn nor mirrored with the base.
    let marked = |t: &mut Term<'_>, implicit: bool| {
        feed(t, b"\x1b[?25l\x1b[2J\x1b[H");
        feed(t, if implicit { b"\x1b[8h" } else { b"\x1b[8l" });
        let text = match implicit {
            true => format!("\x1b[2;1H{alef}\u{2211}\u{301}"),
            false => format!("\x1b[2;{}H\u{2211}\u{301}{alef}", cols - 1),
        };
        feed(t, text.as_bytes());
        feed(t, b"\x1b[10;3HThe quick brown fox jumps 0123456789");
    };
    marked(&mut t, true);
    let got = three(&mut arms, &mut t, "a flipped base with a mark");
    marked(&mut t, false);
    let plain = three(&mut arms, &mut t, "a base with a mark, unflipped");
    let mut whole = plain.clone();
    for y in ch..2 * ch {
        let at = y * w + col * cw;
        whole[at..at + cw].reverse();
    }
    assert!(differing(&got, &plain) > 0, "the base is flipped");
    assert!(differing(&got, &whole) > 0, "the acute is not");
    // The base mirrored: in the cell's pixel rows the acute leaves alone,
    // where the base drawn with it and the base drawn alone agree, the
    // flipped cell is the bare U+2211 mirrored.
    feed(&mut t, b"\x1b[?25l\x1b[2J\x1b[H\x1b[8l");
    feed(
        &mut t,
        format!("\x1b[2;{}H\u{2211}{alef}", cols - 1).as_bytes(),
    );
    feed(&mut t, b"\x1b[10;3HThe quick brown fox jumps 0123456789");
    let bare = three(&mut arms, &mut t, "the base alone, unflipped");
    let line = |f: &[u32], y: usize| f[y * w + col * cw..y * w + (col + 1) * cw].to_vec();
    let (mut alone, mut tested) = (0, 0);
    for y in ch..2 * ch {
        if line(&plain, y) != line(&bare, y) {
            continue;
        }
        alone += 1;
        let mut mirrored = line(&bare, y);
        mirrored.reverse();
        tested += usize::from(mirrored != line(&bare, y));
        assert_eq!(line(&got, y), mirrored, "the base mirrored, pixel row {y}");
    }
    assert!(alone < ch, "the control: the acute inks a row of the cell");
    assert!(
        tested > 0,
        "the control: a row only the base inks is not its own mirror"
    );
}

/// **A reversed string draws alike on every arm**: with BDSM reset an SRS
/// string is drawn in reverse inside its line (`[[source: ecma48]]`
/// 8.3.137, TR/53's rule), the same pixels on the cell, copied and CPU arms
/// as the same text written in its visual order; the control, the text
/// with no string, differs.
#[test]
fn a_reversed_string_draws_alike_on_every_arm() {
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let built = read("baud-app/fonts/built/Regular.ttf");
    let noto = read("baud-font/tests/fonts/NotoColorEmoji-subset.ttf");
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&noto).expect("parse"),
    ];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let three = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, what: &str| {
        arms.reads = Reads::Copy;
        let (a, _) = arms.draw(t);
        let b = arms.draw_copied(t);
        let c = cpu_frame(arms, t);
        assert_eq!(differing(&a, &b), 0, "{what}: the cell arm differs");
        assert_eq!(differing(&a, &c), 0, "{what}: the CPU arm differs");
        a
    };
    let prelude = |t: &mut Term<'_>| {
        feed(
            t,
            b"\x1bc\x1b[?25l\x1b[8;1HThe quick brown fox jumps 0123456789",
        );
    };
    // BDSM reset: `ab` and the reversed string `cde`, then `f`.
    {
        arms!(fresh() => arms, t);
        prelude(&mut t);
        feed(&mut t, b"\x1b[8l\x1b[2;1Hab\x1b[1[cde\x1b[0[f\x1b[4;1H");
        let got = three(&mut arms, &mut t, "SRS, explicit");
        prelude(&mut t);
        feed(&mut t, b"\x1b[8l\x1b[2;1Habedcf\x1b[4;1H");
        let want = three(&mut arms, &mut t, "written in visual order");
        assert_eq!(differing(&got, &want), 0, "the reversed string drawn");
        prelude(&mut t);
        feed(&mut t, b"\x1b[8l\x1b[2;1Habcdef\x1b[4;1H");
        let stored = three(&mut arms, &mut t, "no string");
        assert!(differing(&stored, &want) > 0, "the control: as written");
    }
}

/// **A row DECRLM wrote draws alike on every arm**: with BDSM set it is
/// drawn as its cells hold (`[[source: vt510-decrlm]]`), the same pixels on
/// the cell, copied and CPU arms as the same cells drawn with BDSM reset;
/// the control, the same cells written in order with BDSM set, is reordered
/// by UAX #9.
#[test]
fn a_leftward_row_draws_alike_on_every_arm() {
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let built = read("baud-app/fonts/built/Regular.ttf");
    let noto = read("baud-font/tests/fonts/NotoColorEmoji-subset.ttf");
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&noto).expect("parse"),
    ];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let three = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, what: &str| {
        arms.reads = Reads::Copy;
        let (a, _) = arms.draw(t);
        let b = arms.draw_copied(t);
        let c = cpu_frame(arms, t);
        assert_eq!(differing(&a, &b), 0, "{what}: the cell arm differs");
        assert_eq!(differing(&a, &c), 0, "{what}: the CPU arm differs");
        a
    };
    let prelude = |t: &mut Term<'_>| {
        feed(
            t,
            b"\x1bc\x1b[?25l\x1b[8;1HThe quick brown fox jumps 0123456789",
        );
    };
    // BDSM set: Hebrew and `ab` written leftward from the line's end.
    {
        let cols = usize::from(WS.ws_col);
        arms!(fresh() => arms, t);
        prelude(&mut t);
        feed(
            &mut t,
            format!("\x1b[?34h\x1b[2;{cols}H\u{5D0}\u{5D1} ab\x1b[?34l\x1b[4;1H").as_bytes(),
        );
        let got = three(&mut arms, &mut t, "DECRLM, implicit");
        prelude(&mut t);
        feed(
            &mut t,
            format!("\x1b[8l\x1b[2;{}Hba \u{5D1}\u{5D0}\x1b[4;1H", cols - 4).as_bytes(),
        );
        let want = three(&mut arms, &mut t, "the cells drawn as stored, BDSM reset");
        assert_eq!(differing(&got, &want), 0, "the row drawn as its cells hold");
        prelude(&mut t);
        feed(
            &mut t,
            format!("\x1b[2;{}Hba \u{5D1}\u{5D0}\x1b[4;1H", cols - 4).as_bytes(),
        );
        let reordered = three(&mut arms, &mut t, "the same cells written in order");
        assert!(
            differing(&reordered, &want) > 0,
            "the control: UAX #9 reorders the cells written in order"
        );
    }
}

/// **A line SCP turns draws alike on every arm** (`[[source: ecma48]]`
/// 8.3.111): with BDSM set, an all-Latin line SCP 2 sets right to left is
/// drawn as the same text written in its visual order against the right
/// edge, though no cell of it opens a right-to-left run; with BDSM reset a
/// line SCP 2 sets is drawn in reverse, its bracket mirrored and its wide
/// character's cells in order (terminal-wg's explicit RTL,
/// `[[source: terminal-wg-bidi-basic-modes]]`), before and after the
/// renderer has seen a joining letter, which moves the cell arm onto the
/// kept layout; and a bar cursor there is drawn on its cell's right side.
#[test]
fn a_line_scp_turns_draws_alike_on_every_arm() {
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let built = read("baud-app/fonts/built/Regular.ttf");
    let noto = read("baud-font/tests/fonts/NotoColorEmoji-subset.ttf");
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&noto).expect("parse"),
    ];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let cols = usize::from(WS.ws_col);
    let three = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, what: &str| {
        arms.reads = Reads::Copy;
        let (a, _) = arms.draw(t);
        let b = arms.draw_copied(t);
        let c = cpu_frame(arms, t);
        assert_eq!(differing(&a, &b), 0, "{what}: the cell arm differs");
        assert_eq!(differing(&a, &c), 0, "{what}: the CPU arm differs");
        a
    };
    // The glyphs the harness requires of a scene, on a row of their own,
    // after a reset, with the cursor hidden.
    let prelude = |t: &mut Term<'_>, joining: bool| {
        feed(
            t,
            b"\x1bc\x1b[?25l\x1b[8;1HThe quick brown fox jumps 0123456789",
        );
        if joining {
            feed(t, "\x1b[6;1H\u{628}\u{628}\u{628}".as_bytes());
        }
    };
    // Implicit: `abc def!` at level 1 is `!abc def` at the right edge,
    // with autodetection off, under which the path is the level.
    {
        arms!(fresh() => arms, t);
        prelude(&mut t, false);
        feed(&mut t, b"\x1b[2;1H\x1b[?2501l\x1b[2 kabc def!\x1b[4;1H");
        let got = three(&mut arms, &mut t, "SCP 2, implicit");
        prelude(&mut t, false);
        feed(&mut t, format!("\x1b[2;{}H!abc def", cols - 7).as_bytes());
        let want = three(&mut arms, &mut t, "written in visual order");
        assert_eq!(differing(&got, &want), 0, "the visual order drawn");
        prelude(&mut t, false);
        feed(&mut t, b"\x1b[2;1Habc def!");
        let stored = three(&mut arms, &mut t, "first strong");
        assert!(differing(&stored, &want) > 0, "the control: first strong");
    }
    // `?2500`: a box on an explicit right-to-left line, reversed, is drawn
    // closed, each corner as the other side's.
    {
        arms!(fresh() => arms, t);
        prelude(&mut t, false);
        feed(
            &mut t,
            "\x1b[8l\x1b[2;1H\x1b[?2500h\x1b[2 k\u{250C}\u{2500}\u{2510}\x1b[4;1H".as_bytes(),
        );
        let got = three(&mut arms, &mut t, "?2500, explicit");
        prelude(&mut t, false);
        feed(
            &mut t,
            format!(
                "\x1b[8l\x1b[2;{}H\u{250C}\u{2500}\u{2510}\x1b[4;1H",
                cols - 2
            )
            .as_bytes(),
        );
        let want = three(&mut arms, &mut t, "the box written in visual order");
        assert_eq!(differing(&got, &want), 0, "the box drawn closed");
        prelude(&mut t, false);
        feed(
            &mut t,
            "\x1b[8l\x1b[2;1H\x1b[2 k\u{250C}\u{2500}\u{2510}\x1b[4;1H".as_bytes(),
        );
        let open = three(&mut arms, &mut t, "without ?2500");
        assert!(
            differing(&open, &want) > 0,
            "the control: the corners face out"
        );
    }
    // Explicit: `ab(` and a wide character, reversed whole.
    for joining in [false, true] {
        let what = |s: &str| format!("{s}, joining letter seen: {joining}");
        arms!(fresh() => arms, t);
        // Twice where a joining letter is on the screen: the first frame
        // sees it, and the second is drawn by the kept layout.
        let draw = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, s: &str| {
            if joining {
                three(arms, t, &what(&format!("{s}, first")));
            }
            three(arms, t, &what(s))
        };
        prelude(&mut t, joining);
        feed(&mut t, "\x1b[8l\x1b[2;1H\x1b[2 kab(\u{4E00}".as_bytes());
        let got = draw(&mut arms, &mut t, "SCP 2, explicit");
        assert!(arms.renderer.bidi().reordered(Row::from_index(1)));
        // The bar cursor on `a`, drawn at the right edge on its right side.
        feed(&mut t, b"\x1b[6 q\x1b[2;1H\x1b[?25h");
        let bar = draw(&mut arms, &mut t, "SCP 2, explicit, bar");
        let w = usize::try_from(arms.renderer.whole_size().0.value()).unwrap();
        let cw = usize::try_from(arms.renderer.metrics().cell_w.value()).unwrap();
        let ch = usize::try_from(arms.renderer.metrics().cell_h.value()).unwrap();
        let changed: Vec<(usize, usize)> = (0..bar.len())
            .filter(|&i| bar[i] != got[i])
            .map(|i| (i % w, i / w))
            .collect();
        let (left, right) = ((cols - 1) * cw, cols * cw);
        assert!(
            changed
                .iter()
                .all(|&(x, y)| x >= left && x < right && (ch..2 * ch).contains(&y)),
            "{}",
            what("the bar in the last column")
        );
        let xs = |x: usize| changed.iter().any(|&(cx, _)| cx == x);
        assert!(xs(right - 1) && !xs(left), "{}", what("on the right side"));
        prelude(&mut t, joining);
        feed(
            &mut t,
            format!("\x1b[8l\x1b[2;{}H\u{4E00})ba", cols - 4).as_bytes(),
        );
        let want = draw(&mut arms, &mut t, "written in visual order");
        assert_eq!(differing(&got, &want), 0, "{}", what("the reverse drawn"));
    }
}

/// **SAPV's variants draw alike on every arm, and as the characters they
/// stand for**: Arabic-Indic digits (2), a decimal comma (10), a vowel
/// hidden (16) and, with BDSM reset, a right-to-left line's brackets left
/// as stored (15), each pixel for pixel on the cell, instance and CPU arms,
/// and against the same rows written with the characters drawn.
#[test]
fn presentation_variants_draw_alike_on_every_arm() {
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let built = read("baud-app/fonts/built/Regular.ttf");
    let italic = read("baud-app/fonts/built/Italic.ttf");
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&italic).expect("parse"),
    ];
    let roles = [FaceRole::Fallback, FaceRole::Style(FaceStyle::Italic)];
    let fresh = || {
        Renderer::new(
            Stack::with_roles(&faces, &roles).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let cols = usize::from(WS.ws_col);
    // A right-to-left path with BDSM reset, at level 1 throughout: italic
    // Latin, brackets, a vav, a square root L4 flips and a lam-alef pair,
    // each sheared, the brackets as mirrors; and the same on a line BDSM
    // lays out, a Hebrew word with a Latin one at level 2 between.
    const SLANTED: &str = "\x1b[8l\x1b[5;1H\x1b[2 k\x1b[3m\x1b[17 ]l(ab)\u{5D5}\u{221A}\
         \u{644}\u{627}\x1b[0m\x1b[0 ]\x1b[5;1H\x1b[8h\x1b[6;1H\x1b[3m\x1b[17 ]\
         \u{5E9}\u{5DC}\u{5D5}\u{5DD} ok \u{5D5}(\u{221A})\x1b[0m\x1b[0 ]";
    let three = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, what: &str| {
        arms.reads = Reads::Copy;
        let (a, _) = arms.draw(t);
        let b = arms.draw_copied(t);
        let c = cpu_frame(arms, t);
        assert_eq!(differing(&a, &b), 0, "{what}: the cell arm differs");
        assert_eq!(differing(&a, &c), 0, "{what}: the CPU arm differs");
        a
    };
    let varied = format!(
        "\x1b[?25l\x1b[2J\x1b[H\x1b[2 ]12\x1b[0 ] \x1b[10 ]3.14\x1b[0 ]\r\n\
         \x1b[16 ]\u{5D1}\u{5BC}\u{5B0}\x1b[0 ]\r\n\
         \x1b[8l\x1b[3;1H\x1b[2 k\x1b[15 ](ab)\x1b[0 ]\x1b[3;1H\x1b[8h\
         {SLANTED}\
         \x1b[10;3HThe quick brown fox jumps 0123456789"
    );
    let written = format!(
        "\x1bc\x1b[?25l\x1b[2J\x1b[H\u{661}\u{662} 3,14\r\n\
         \u{5D1}\u{5BC}\r\n\
         \x1b[8l\x1b[3;{}H)ba(\x1b[3;1H\x1b[8h\
         {SLANTED}\
         \x1b[10;3HThe quick brown fox jumps 0123456789",
        cols - 3
    );
    arms!(fresh() => arms, t);
    feed(&mut t, varied.as_bytes());
    // The third row is drawn reversed with BDSM reset; the frames are
    // taken with it reset so both scenes draw that row the same way.
    feed(&mut t, b"\x1b[8l");
    let got = three(&mut arms, &mut t, "varied");
    feed(&mut t, written.as_bytes());
    feed(&mut t, b"\x1b[8l");
    let want = three(&mut arms, &mut t, "written");
    assert_eq!(
        differing(&got, &want),
        0,
        "drawn as the characters they stand for"
    );
    // The control: without 17 the slanted row draws otherwise.
    let upright = written.replace("\x1b[17 ]", "");
    feed(&mut t, upright.as_bytes());
    feed(&mut t, b"\x1b[8l");
    let leaning_right = three(&mut arms, &mut t, "without 17");
    assert_ne!(differing(&got, &leaning_right), 0, "17 slants the row");
    // With BDSM set, the sixth row's Hebrew, at the level UAX #9 gives it.
    let bdsm = |bytes: &str| format!("\x1bc{bytes}\x1b[8h");
    feed(&mut t, bdsm(&varied).as_bytes());
    let implicit = three(&mut arms, &mut t, "with BDSM set");
    feed(&mut t, bdsm(&varied.replace("\x1b[17 ]", "")).as_bytes());
    let plain = three(&mut arms, &mut t, "with BDSM set, without 17");
    assert_ne!(differing(&implicit, &plain), 0, "17 slants with BDSM set");
}

/// **A composition of Arabic is drawn joined, as the same text written is,
/// on every arm** (`[[source: unicode-ch9]]` 9.2.2: "a display process
/// must select an appropriate glyph to depict each Arabic letter according
/// to its immediate joining context"): the row is laid out with the
/// composition written in it, so its letters take the forms that text
/// written takes, joined with one another and with the letters written
/// either side of it, which take theirs from it; a lam and an alef draw
/// their ligature, composed together or one beside the other written. Each
/// on Courier New, whose forms `arabic_letters_draw_joined_on_every_arm`
/// holds to the presentation forms, with BDSM set and reset, on a line
/// drawn in order and in reverse, single and double width, beside a block
/// cursor and a bar, on a renderer that has drawn no joining letter and on
/// one that has -- with BDSM reset the layout then keeps the last frame's
/// joins and finds them again around the composition. Beside a letter
/// underlined in another colour too, whose line the cell arm draws after
/// the records inside their rows and before the rest, as the instance arm
/// draws a row's lines after its glyphs: the composition's glyphs are
/// among the first. And in a rendition SAPV 8 gives the cell it starts on,
/// held by 21, each letter in its final form, as written in it
/// (`[[source: ecma48]]` 8.3.108). The reference is the text written
/// underlined at the cursor, the cursor after it, and every arm equal to
/// it.
#[test]
fn a_composition_of_arabic_draws_joined_as_written_on_every_arm() {
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let built = read("baud-app/fonts/built/Regular.ttf");
    let courier = std::fs::read("/System/Library/Fonts/Supplemental/Courier New.ttf")
        .expect("Courier New, which macOS ships; this test cannot skip itself");
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&courier).expect("parse"),
    ];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    // Every case's failures, listed together at the end.
    let failed = std::cell::RefCell::new(Vec::<String>::new());
    let fail = |what: String| failed.borrow_mut().push(what);
    let four = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, what: &str| {
        arms.reads = Reads::Copy;
        let (a, live) = arms.draw(t);
        let b = arms.draw_copied(t);
        let c = cpu_frame(arms, t);
        for (arm, f) in [("cell", &live), ("copied cell", &b), ("CPU", &c)] {
            let d = differing(&a, f);
            if d > 0 {
                fail(format!("{what}: the {arm} arm differs by {d} pixels"));
            }
        }
        a
    };
    let (beh, yeh, teh, lam, alef) = ("\u{628}", "\u{64A}", "\u{62A}", "\u{644}", "\u{627}");
    // Text enough that the map holds the scene's glyphs.
    let fox = "\x1b[5;3HThe quick brown fox jumps 0123456789";
    let bayt = format!("{beh}{yeh}{teh}");
    // What is written before the cursor, the cursor's column, and what
    // is composed there: a word alone; its last letter after the two
    // before it; its middle letter between the two, over a space; a
    // lam-alef; an alef after a lam written; a lam over a space before an
    // alef.
    let words = [
        (String::new(), 3, bayt.clone()),
        (format!("\x1b[3G{beh}{yeh}"), 5, teh.to_string()),
        (format!("\x1b[3G{beh} {teh}"), 4, yeh.to_string()),
        (String::new(), 3, format!("{lam}{alef}")),
        (format!("\x1b[3G{lam}"), 4, alef.to_string()),
        (format!("\x1b[3G {alef}"), 3, lam.to_string()),
        (
            format!("\x1b[3G{beh} \x1b[4;58:5:1m{yeh}\x1b[24;59m"),
            4,
            teh.to_string(),
        ),
    ];
    // BDSM set; reset, the line in order; reset, the line right to left.
    let modes = ["\x1b[8h", "\x1b[8l", "\x1b[8l\x1b[2 k"];
    for primed in [false, true] {
        for mode in modes {
            // Whether any case on this layout joined: the premise.
            let mut joined = false;
            for (size, cursor) in ["", "\x1b#6"]
                .into_iter()
                .flat_map(|s| ["\x1b[2 q", "\x1b[6 q"].map(move |c| (s, c)))
            {
                for (before, at, text) in &words {
                    arms!(fresh() => arms, t);
                    if primed {
                        feed(&mut t, format!("\x1b[1;1H{beh}{beh}{beh}{fox}").as_bytes());
                        let _ = four(&mut arms, &mut t, "a joining letter drawn");
                        assert_eq!(arms.renderer.joins_seen(), baud_render::Joins::Seen);
                    }
                    let what = format!("{primed} {mode:?} {size:?} {cursor:?} {before:?} {text:?}");
                    let scene = format!(
                        "\x1bc{cursor}{mode}\x1b[3;1H{size}{before}\x1b[2;3HThe quick brown fox jumps 0123456789\x1b[3;{at}H"
                    );
                    joined |=
                        composed_as_written(&mut arms, &mut t, (&scene, text), &what, &four, &fail);
                }
            }
            assert!(
                joined,
                "{primed} {mode:?}: the premise: a composition joins"
            );
        }
        // SAPV 8 held by 21 on the cells the composition covers: each
        // letter final.
        for mode in modes {
            arms!(fresh() => arms, t);
            if primed {
                feed(&mut t, format!("\x1b[1;1H{beh}{beh}{beh}{fox}").as_bytes());
                let _ = four(&mut arms, &mut t, "a joining letter drawn");
            }
            let what = format!("{primed} {mode:?} SAPV 8");
            let scene = format!(
                "\x1bc\x1b[2 q{mode}\x1b[5;3HThe quick brown fox jumps 0123456789\x1b[3;3H\x1b[21 ]\x1b[8 ]   \x1b[3D"
            );
            composed_as_written(&mut arms, &mut t, (&scene, &bayt), &what, &four, &fail);
            // The control: SAPV 8's forms are not the word's.
            feed(&mut t, scene.as_bytes());
            feed(
                &mut t,
                format!("\x1b[22 ]\x1b[0 ]\x1b[4m{bayt}\x1b[24m").as_bytes(),
            );
            let joined = four(&mut arms, &mut t, &format!("{what}: joined"));
            feed(&mut t, scene.as_bytes());
            feed(&mut t, format!("\x1b[4m{bayt}\x1b[24m").as_bytes());
            let finals = four(&mut arms, &mut t, &format!("{what}: finals"));
            assert!(
                differing(&joined, &finals) > 0,
                "{what}: SAPV 8 draws finals"
            );
        }
    }
    // **Another composition over the same cells of a line that joins**:
    // drawn as it is written, not as the last was -- the line's records
    // are not kept from a frame the composition differed in.
    for size in ["", "\x1b#6"] {
        arms!(fresh() => arms, t);
        let what = format!("{size:?} recomposed");
        let scene = format!("\x1bc\x1b[2 q\x1b[8h\x1b[3;1H{size}{fox}\x1b[3;3H");
        feed(&mut t, scene.as_bytes());
        let (row, col) = t.cursor();
        arms.composition = Some(Composition {
            row,
            col,
            text: bayt
                .chars()
                .map(baud_domain::Codepoint::from_char)
                .collect(),
            measure: baud_render::Measure::default(),
            hints: Vec::new(),
            caret: Caret::End,
        });
        let _ = four(&mut arms, &mut t, &format!("{what}: first"));
        let other = format!("{teh}{yeh}{beh}");
        if let Some(c) = arms.composition.as_mut() {
            c.text = other
                .chars()
                .map(baud_domain::Codepoint::from_char)
                .collect();
        }
        let composed = four(&mut arms, &mut t, &format!("{what}: again"));
        arms.composition = None;
        feed(&mut t, format!("\x1b[4m{other}\x1b[24m").as_bytes());
        let written = four(&mut arms, &mut t, &format!("{what}: written"));
        let d = differing(&composed, &written);
        if d > 0 {
            fail(format!(
                "{what}: as the same text written underlined, {d} pixels"
            ));
        }
    }
    let failed = failed.into_inner();
    assert!(
        failed.is_empty(),
        "{} cases:\n{}",
        failed.len(),
        failed.join("\n")
    );
}

/// `scene` fed, `text` composed at its cursor, against `text` written
/// there underlined: equal, and each against the frame without it, and
/// the composition's row joined as the written row is -- each joined
/// cell's form and pair, and the letters and forms of its word. What
/// differs is told to `fail`; the answer is whether the row joined.
fn composed_as_written(
    arms: &mut Arms<'_, '_>,
    t: &mut Term<'_>,
    (scene, text): (&str, &str),
    what: &str,
    frame: &dyn Fn(&mut Arms<'_, '_>, &mut Term<'_>, &str) -> Vec<u32>,
    fail: &dyn Fn(String),
) -> bool {
    feed(t, scene.as_bytes());
    let (row, col) = t.cursor();
    arms.composition = Some(Composition {
        row,
        col,
        text: text
            .chars()
            .map(baud_domain::Codepoint::from_char)
            .collect(),
        measure: baud_render::Measure::default(),
        hints: Vec::new(),
        caret: Caret::End,
    });
    let composed = frame(arms, t, &format!("{what}: composed"));
    // Each joined cell of the row: its column, form and pair, and its
    // word's letters and forms either side.
    let joins = |arms: &Arms<'_, '_>| {
        let letters = |ws: &[baud_render::WordCell]| -> Vec<_> {
            ws.iter()
                .map(|w| (w.cell.content.as_codepoint(), w.form))
                .collect()
        };
        arms.renderer.bidi().row_joins(row).map(|rj| {
            rj.cells()
                .iter()
                .enumerate()
                .filter_map(|(v, j)| {
                    let (b, a) = rj.around(j.as_ref()?);
                    j.map(|j| (v, j.form, j.pair, letters(b), letters(a)))
                })
                .collect::<Vec<_>>()
        })
    };
    let laid = joins(arms);
    arms.composition = None;
    let plain = frame(arms, t, &format!("{what}: plain"));
    feed(t, format!("\x1b[4m{text}\x1b[24m").as_bytes());
    let written = frame(arms, t, &format!("{what}: written"));
    if laid != joins(arms) {
        fail(format!("{what}: joined otherwise than written"));
    }
    assert!(differing(&composed, &plain) > 0, "{what}: drawn");
    let d = differing(&composed, &written);
    if d > 0 {
        fail(format!(
            "{what}: as the same text written underlined, {d} pixels"
        ));
    }
    laid.is_some()
}

/// **Arabic DECRLM wrote joins as written, and takes the forms SAPV asks,
/// alike on every arm** (`[[source: ecma48]]` 8.3.108;
/// `[[source: vt510-decrlm]]`): with BDSM set the row is drawn as its cells
/// hold and its letters are read from the right, the order the program sent
/// them, so it draws as the same cells with BDSM reset, on the cell, copied
/// and CPU arms; the control, the word written with no form asked, draws
/// otherwise. On Courier New, whose forms
/// `arabic_letters_draw_joined_on_every_arm` holds.
#[test]
fn arabic_decrlm_wrote_draws_joined_with_its_forms_on_every_arm() {
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let built = read("baud-app/fonts/built/Regular.ttf");
    let courier = std::fs::read("/System/Library/Fonts/Supplemental/Courier New.ttf")
        .expect("Courier New, which macOS ships; this test cannot skip itself");
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&courier).expect("parse"),
    ];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let three = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, what: &str| {
        arms.reads = Reads::Copy;
        let (a, _) = arms.draw(t);
        let b = arms.draw_copied(t);
        let c = cpu_frame(arms, t);
        assert_eq!(differing(&a, &b), 0, "{what}: the cell arm differs");
        assert_eq!(differing(&a, &c), 0, "{what}: the CPU arm differs");
        a
    };
    let cols = usize::from(WS.ws_col);
    let word = |forms: &str| {
        format!("\x1b[?34h\x1b[2;{cols}H{forms}\u{628}\u{64A}\u{62A}\x1b[?34l\x1b[4;1H")
    };
    let scene = |t: &mut Term<'_>, bdsm: &str, forms: &str| {
        feed(
            t,
            format!("\x1bc\x1b[?25l\x1b[8;1HThe quick brown fox jumps 0123456789{bdsm}").as_bytes(),
        );
        feed(t, word(forms).as_bytes());
    };
    for forms in ["", "\x1b[5 ]", "\x1b[8 ]"] {
        arms!(fresh() => arms, t);
        scene(&mut t, "", forms);
        let got = three(&mut arms, &mut t, &format!("BDSM set, {forms:?}"));
        scene(&mut t, "\x1b[8l", forms);
        let want = three(&mut arms, &mut t, &format!("BDSM reset, {forms:?}"));
        assert_eq!(differing(&got, &want), 0, "{forms:?}: joined as written");
        if !forms.is_empty() {
            scene(&mut t, "", "");
            let plain = three(&mut arms, &mut t, "no form asked");
            assert_ne!(differing(&got, &plain), 0, "{forms:?}: the form drawn");
        }
    }
}

/// **A paragraph inside DECSLRM side margins draws alike on every arm**:
/// twenty Hebrew letters and "12" written inside margins 101 to 120, the
/// digits wrapped to the next line's left margin, with "xy" and "zw" left
/// of the margins, are laid out inside them as one paragraph and the
/// letters outside them where they were written (`[[source:
/// vt510-decslrm]]`, `[[source: uax9-bidi]]`) -- the same pixels on the
/// cell, copied and CPU arms as the same cells written in that visual
/// order with BDSM reset. The margins end at the screen's edge, the one
/// wrap the core marks as continuing (since 2026-11-03). The control: the
/// same letters written with no margins, each row read whole, draws
/// otherwise. Written from the bottom margin, the last row, so the wrap
/// scrolls the rectangle, the paragraph draws so on the last two rows.
#[test]
fn a_paragraph_inside_side_margins_draws_alike_on_every_arm() {
    let read =
        |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).expect("face");
    let built = read("baud-app/fonts/built/Regular.ttf");
    let noto = read("baud-font/tests/fonts/NotoColorEmoji-subset.ttf");
    let faces = [
        Face::parse(&built).expect("parse"),
        Face::parse(&noto).expect("parse"),
    ];
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let three = |arms: &mut Arms<'_, '_>, t: &mut Term<'_>, what: &str| {
        arms.reads = Reads::Copy;
        let (a, _) = arms.draw(t);
        let b = arms.draw_copied(t);
        let c = cpu_frame(arms, t);
        assert_eq!(differing(&a, &b), 0, "{what}: the cell arm differs");
        assert_eq!(differing(&a, &c), 0, "{what}: the CPU arm differs");
        a
    };
    let he: String = (0..20)
        .map(|i| char::from_u32(0x5D0 + i).unwrap())
        .collect();
    let back: String = he.chars().rev().collect();
    arms!(fresh() => arms, t);
    feed(
        &mut t,
        format!(
            "\x1bc\x1b[?25l\x1b[?69h\x1b[101;120s\x1b[1;101H{he}12\x1b[1;1Hxy\x1b[2;1Hzw\x1b[4;1H"
        )
        .as_bytes(),
    );
    let got = three(&mut arms, &mut t, "inside the margins");
    feed(
        &mut t,
        format!(
            "\x1bc\x1b[?25l\x1b[8l\x1b[1;1Hxy\x1b[1;101H{back}\x1b[2;1Hzw\x1b[2;119H12\x1b[4;1H"
        )
        .as_bytes(),
    );
    let want = three(&mut arms, &mut t, "the visual order, BDSM reset");
    assert_eq!(differing(&got, &want), 0, "laid out inside the margins");
    feed(
        &mut t,
        format!("\x1bc\x1b[?25l\x1b[1;101H{he}\x1b[2;101H12\x1b[1;1Hxy\x1b[2;1Hzw\x1b[4;1H")
            .as_bytes(),
    );
    let whole = three(&mut arms, &mut t, "no margins");
    assert!(differing(&whole, &want) > 0, "the control: read whole");
    // Written from the bottom margin, the wrap to the digits scrolls the
    // rectangle a row up and the margins move with it.
    feed(
        &mut t,
        format!(
            "\x1bc\x1b[?25l\x1b[?69h\x1b[101;120s\x1b[10;101H{he}12\x1b[9;1Hxy\x1b[10;1Hzw\x1b[1;1H"
        )
        .as_bytes(),
    );
    let scrolled = three(&mut arms, &mut t, "scrolled inside the margins");
    feed(
        &mut t,
        format!(
            "\x1bc\x1b[?25l\x1b[8l\x1b[9;1Hxy\x1b[9;101H{back}\x1b[10;1Hzw\x1b[10;119H12\x1b[1;1H"
        )
        .as_bytes(),
    );
    let want = three(&mut arms, &mut t, "the visual order on the last two rows");
    assert_eq!(differing(&scrolled, &want), 0, "scrolled with its margins");
}

/// **A syllable is drawn alike on every arm**: the README's Hindi line,
/// टर्मिनल हर अक्षर को ग्रिड में रखता है।, in Kohinoor Devanagari behind
/// JetBrains Mono. With `?2027` reset each letter and spacing vowel sign
/// takes a cell and every word is drawn whole, its syllables re-formed from
/// the cells the core parted them into -- by the instance arm's runs, by
/// records over masked cells on the cell arm, and from the instance list
/// on the CPU arm; with spacing marks joined, the same over fewer cells, a
/// vowel sign in its consonant's cell; with `?2027` set, a whole syllable
/// in one cell. A
/// syllable a soft wrap parts, its vowel sign alone at the next row's
/// start, is drawn as two. The premises: the syllables went to the
/// records, and the three frames differ, the columns having moved; a line
/// of Latin keeps the map's own premise, that it holds the scene's glyphs.
///
/// Kohinoor has no bold face of its own here, so its bold line draws from
/// the regular face; the line and its bold are drawn again on the app's
/// stack (`app_stack`), whose built Bold draws Devanagari, under each
/// count, each the first frame of a renderer of its own (`first_frame`),
/// so an arm that drew a bold syllable from the regular face would draw
/// another glyph than the instance arm's.
#[test]
fn a_syllable_draws_alike_on_every_arm() {
    let data = std::fs::read(format!(
        "{}/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("face");
    let deva = std::fs::read("/System/Library/Fonts/Kohinoor.ttc").expect("Kohinoor Devanagari");
    let tamil = std::fs::read("/System/Library/Fonts/Supplemental/Tamil Sangam MN.ttc")
        .expect("Tamil Sangam MN");
    let faces = [
        Face::parse(&data).expect("parse"),
        Face::parse_index(&deva, baud_font::FaceIndex::of(0u32)).expect("parse"),
        Face::parse_index(&tamil, baud_font::FaceIndex::of(0u32)).expect("parse"),
    ];
    let roles = [FaceRole::Fallback, FaceRole::Fallback, FaceRole::Fallback];
    let renderer = Renderer::new(
        Stack::with_roles(&faces, &roles).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    arms!(renderer => arms, t);
    let line = "टर्मिनल हर अक्षर को ग्रिड में रखता है।";
    // ट, र् and म in the last three columns, ि at the next row's start.
    let wrapped = format!("\x1b[4;{}H{line}", usize::from(WS.ws_col) - 2);
    let mut frames = Vec::new();
    for (setup, marks) in [
        ("", baud_domain::SpacingMarks::Column),
        ("", baud_domain::SpacingMarks::Joined),
        ("\x1b[?2027h", baud_domain::SpacingMarks::Column),
    ] {
        t.set_spacing_marks(marks);
        feed(
            &mut t,
            format!(
                "\x1bc\x1b[?25l{setup}{line}\r\n\x1b[1m{line}\x1b[m\r\n\
                 the quick brown fox jumps over 0123456789{wrapped}"
            )
            .as_bytes(),
        );
        let (a, b) = arms.draw(&mut t);
        let cpu = cpu_frame(&mut arms, &mut t);
        let stats = arms.exceptions.stats();
        assert!(
            stats.cells.value() >= 12,
            "{setup:?} {marks:?}: the syllables went to the records: {stats:?}"
        );
        assert_eq!(
            differing(&a, &b),
            0,
            "{setup:?} {marks:?}: the GPU arms disagree"
        );
        assert_eq!(
            differing(&a, &cpu),
            0,
            "{setup:?} {marks:?}: the CPU arm disagrees"
        );
        frames.push(a);
    }
    assert!(
        differing(&frames[0], &frames[1]) > 0,
        "the setting moved nothing"
    );
    assert!(differing(&frames[0], &frames[2]) > 0, "?2027 moved nothing");
    t.set_spacing_marks(baud_domain::SpacingMarks::Column);
    // **Never shrunk**: Tamil கொ, KA and the vowel sign O, is two cells
    // whose glyphs in Tamil Sangam MN are wider than the two; a syllable
    // outside a Devanagari word is centred in its cells at its face's
    // size, so they reach into the `|` after them, alike on every arm.
    // Against the same row with the syllable blank, the pixels that differ
    // reach past its two cells.
    let row = |s: &str| format!("\x1bc\x1b[?25l\x1b[2;1H{s}|");
    feed(&mut t, row("கொ").as_bytes());
    let (with, with_cell) = arms.draw(&mut t);
    let with_cpu = cpu_frame(&mut arms, &mut t);
    feed(&mut t, row("  ").as_bytes());
    let (blank, _) = arms.draw(&mut t);
    assert_eq!(
        differing(&with, &with_cell),
        0,
        "the GPU arms disagree on கொ"
    );
    assert_eq!(
        differing(&with, &with_cpu),
        0,
        "the CPU arm disagrees on கொ"
    );
    let (fw, _) = arms.renderer.whole_size();
    let m = arms.renderer.metrics();
    let cell = (
        usize::try_from(m.cell_w.value()).unwrap(),
        usize::try_from(m.cell_h.value()).unwrap(),
    );
    let reach = diff_box(&with, &blank, usize::try_from(fw.value()).unwrap(), cell);
    assert!(
        reach.is_some_and(|(c0, _, c1, _)| c0 == 0 && c1 >= 2),
        "கொ is drawn at its face's size, past its two cells: {reach:?}"
    );
    // **On the app's stack**, the bold line in the built Bold.
    let files = app_files();
    let (faces, roles) = app_stack(&files);
    let mut renderer = Renderer::new(
        Stack::with_roles(&faces, &roles).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let cps: Vec<Codepoint> = line.chars().map(Codepoint::from_char).collect();
    assert_ne!(
        renderer.face_for(&cps, FaceStyle::Bold),
        renderer.face_for(&cps, FaceStyle::Regular),
        "the premise: the bold face draws the line's Devanagari"
    );
    for (setup, marks) in [
        ("", baud_domain::SpacingMarks::Column),
        ("", baud_domain::SpacingMarks::Joined),
        ("\x1b[?2027h", baud_domain::SpacingMarks::Column),
    ] {
        let what = format!("{setup:?} {marks:?} on the app's stack");
        first_frame_with(
            (&faces, &roles),
            |t| t.set_spacing_marks(marks),
            &format!("\x1b[?25l{setup}{line}\r\n\x1b[1m{line}\x1b[m"),
            (0, 0),
            &what,
        );
    }
}

/// **A Devanagari word, the block cursor on each of its cells and a
/// selection of each of its syllables, draw alike on every arm**
/// (`baud-render/src/word.rs`): the README's Hindi line, each word shaped
/// whole and laid in its columns, the block on every cell of टर्मिनल and
/// अक्षर lighting the syllable that holds the cell, and each of their
/// syllables selected alone -- under the default count, spacing marks
/// joined and `?2027`, in Kohinoor Devanagari behind JetBrains Mono, and,
/// where the README's font cache holds it, in Noto Sans Devanagari's
/// variable file with its narrower instances. The block a word's syllable
/// takes is the records', the cell shader drawing none
/// (`Overlay::cursor_drawn`); the premises: the words went to the records,
/// and the block on two syllables draws two frames.
#[test]
fn a_devanagari_word_and_its_lights_draw_alike_on_every_arm() {
    const SHA: &str = "9f82923465ef82fb86c6b99cbdd4947ba832fe3335948de815b18adfd75947c7";
    let data = std::fs::read(format!(
        "{}/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("face");
    let deva = std::fs::read("/System/Library/Fonts/Kohinoor.ttc").expect("Kohinoor Devanagari");
    let cache = std::env::var_os("NOTO_CACHE")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_CACHE_HOME")
                .map(|d| std::path::PathBuf::from(d).join("baud-readme"))
        })
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache/baud-readme"))
        });
    let noto = cache.and_then(|d| std::fs::read(d.join("NotoSansDevanagari[wdth,wght].ttf")).ok());
    if let Some(n) = &noto {
        assert_eq!(
            sha256::sha256_hex(n),
            SHA,
            "the cache's file is not the pinned one"
        );
    }
    let mut stacks: Vec<(Vec<Face<'_>>, Vec<FaceRole>)> = vec![(
        vec![
            Face::parse(&data).expect("parse"),
            Face::parse_index(&deva, baud_font::FaceIndex::of(0u32)).expect("parse"),
        ],
        vec![FaceRole::Fallback, FaceRole::Fallback],
    )];
    if let Some(n) = &noto {
        let mut faces = vec![Face::parse(&data).unwrap(), Face::parse(n).unwrap()];
        let mut roles = vec![FaceRole::Fallback, FaceRole::Fallback];
        for w in baud_render::narrower_widths(&faces[1], &[]) {
            let mut f = Face::parse(n).unwrap();
            f.set_variations(&[baud_font::var::Variation {
                tag: baud_font::Tag::from_bytes(*b"wdth"),
                value: baud_font::var::Fixed::from_f32(w),
            }]);
            faces.push(f);
            roles.push(FaceRole::Narrower);
        }
        stacks.push((faces, roles));
    }
    let line = "टर्मिनल हर अक्षर को ग्रिड में रखता है।";
    let mut drawn = 0usize;
    for (faces, roles) in &stacks {
        let renderer = Renderer::new(
            Stack::with_roles(faces, roles).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap();
        arms!(renderer => arms, t);
        for (setup, marks) in [
            ("", baud_domain::SpacingMarks::Column),
            ("", baud_domain::SpacingMarks::Joined),
            ("\x1b[?2027h", baud_domain::SpacingMarks::Column),
        ] {
            t.set_spacing_marks(marks);
            let screen = format!(
                "\x1bc\x1b[?25l{setup}\x1b[2;3H{line}\x1b[4;1Hthe quick brown fox 0123456789"
            );
            arms.highlight = None;
            feed(&mut t, screen.as_bytes());
            let _ = three_arms(&mut arms, &mut t, "the line");
            assert!(
                arms.exceptions.stats().cells.value() >= 12,
                "{setup:?} {marks:?}: the words went to the records"
            );
            let words = arms.renderer.words_on(Row::from_index(1)).to_vec();
            assert_eq!(words.len(), 8, "{setup:?} {marks:?}: {words:?}");
            let mut frames = Vec::new();
            for w in [&words[0], &words[2]] {
                for c in w.at.index()..w.at.index() + w.cells.index() {
                    feed(&mut t, format!("\x1b[2;{}H\x1b[?25h", c + 1).as_bytes());
                    let what = format!("{setup:?} {marks:?}: the block on column {c}");
                    frames.push(three_arms(&mut arms, &mut t, &what));
                    assert_eq!(
                        arms.exceptions.cursor_drawn(),
                        baud_render::CursorDrawn::Records,
                        "{what}: the records draw it"
                    );
                    drawn += 1;
                }
                feed(&mut t, b"\x1b[?25l");
                for &(at, n) in &w.syllables {
                    let row = Row::from_index(1);
                    arms.highlight = Some(Highlight::new(
                        (row, at),
                        (row, Col::at_index(at.index() + n.index() - 1)),
                        Region::Stream,
                    ));
                    let what = format!("{setup:?} {marks:?}: column {} selected", at.index());
                    let _ = three_arms(&mut arms, &mut t, &what);
                    drawn += 1;
                }
                arms.highlight = None;
            }
            assert!(
                frames.windows(2).any(|f| differing(&f[0], &f[1]) > 0),
                "{setup:?} {marks:?}: the block moved nothing"
            );
        }
    }
    assert!(drawn >= 60, "{drawn} frames compared");
}

/// **Bold and italic Hindi draw alike on every arm**, in the faces the
/// embedded font's four built styles give them (`baud-app/fonts/built/`): the Hindi line of the README
/// clip's `HELLO` (`baud-app/examples/demo/session.rs`) in regular,
/// `SGR 1`, `SGR 3` and `SGR 1;3`, and bold with `?2027` set,
/// behind the icon face as the app stacks them; a word whose rendition
/// changes inside it, either way; and a bold Latin line, which the map
/// draws. Each is the first frame of a renderer of its own (`first_frame`).
/// While a word took its face from the renderer's own rendition table, which
/// the cell path does not resolve, the cell arm laid bold Hindi in the
/// Regular face, 1371 of the frame's pixels away from the other arms, and
/// italic Hindi in the Regular face's glyphs, which draw the Italic face's
/// pixels, its Devanagari upright. The premises: the words went to the
/// records, the bold faces' frames are not the regular one, and the italic
/// face draws the line's Devanagari with glyphs of its own.
#[test]
fn bold_and_italic_hindi_draw_alike_on_every_arm() {
    let here = |p: &str| {
        std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR")))
            .unwrap_or_else(|e| panic!("{p}: {e}"))
    };
    let files = [
        here("baud-app/fonts/built/Regular.ttf"),
        here("baud-app/fonts/symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf"),
        here("baud-app/fonts/built/Bold.ttf"),
        here("baud-app/fonts/built/Italic.ttf"),
        here("baud-app/fonts/built/BoldItalic.ttf"),
    ];
    let faces: Vec<Face<'_>> = files
        .iter()
        .map(|d| Face::parse(d).expect("parse"))
        .collect();
    let roles = [
        FaceRole::Fallback,
        FaceRole::Fallback,
        FaceRole::Style(FaceStyle::Bold),
        FaceRole::Style(FaceStyle::Italic),
        FaceRole::Style(FaceStyle::BoldItalic),
    ];
    let line = "टर्मिनल हर अक्षर को ग्रिड में रखता है।";
    let mut frames = Vec::new();
    for (name, setup, text, words) in [
        ("regular", "", line, 12),
        ("bold", "\x1b[1m", line, 12),
        ("italic", "\x1b[3m", line, 12),
        ("bold italic", "\x1b[1;3m", line, 12),
        ("bold under ?2027", "\x1b[?2027h\x1b[1m", line, 12),
        ("bold from inside a word", "", "र्\x1b[1mमि", 3),
        ("regular from inside a word", "\x1b[1m", "र्\x1b[mमि", 3),
        ("bold Latin", "\x1b[1m", "the quick brown fox", 0),
    ] {
        let (frame, cells, _) = first_frame(
            (&faces, &roles),
            &format!(
                "\x1b[?25l\x1b[2;3H{setup}{text}\x1b[m\x1b[4;1Hthe quick brown fox 0123456789"
            ),
            (1, 2),
            name,
        );
        assert!(
            cells.value() >= words,
            "{name}: the words went to the records: {cells:?}"
        );
        frames.push(frame);
    }
    assert!(frames[1] != frames[0], "bold Hindi is the Regular's");
    assert!(frames[3] != frames[0], "bold-italic Hindi is the Regular's");
    let mut renderer = Renderer::new(
        Stack::with_roles(&faces, &roles).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let cps: Vec<Codepoint> = line.chars().map(Codepoint::from_char).collect();
    assert_ne!(
        renderer.face_for(&cps, FaceStyle::Italic),
        renderer.face_for(&cps, FaceStyle::Regular),
        "the premise: the italic face draws the line's Devanagari"
    );
}

/// The app's faces: the built Regular, the icon face, and the built Bold,
/// Italic and Bold Italic, each with its role.
fn app_files() -> Vec<(Vec<u8>, FaceRole)> {
    let here = |p: &str| {
        std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR")))
            .unwrap_or_else(|e| panic!("{p}: {e}"))
    };
    vec![
        (here("baud-app/fonts/built/Regular.ttf"), FaceRole::Fallback),
        (
            here("baud-app/fonts/symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf"),
            FaceRole::Fallback,
        ),
        (
            here("baud-app/fonts/built/Bold.ttf"),
            FaceRole::Style(FaceStyle::Bold),
        ),
        (
            here("baud-app/fonts/built/Italic.ttf"),
            FaceRole::Style(FaceStyle::Italic),
        ),
        (
            here("baud-app/fonts/built/BoldItalic.ttf"),
            FaceRole::Style(FaceStyle::BoldItalic),
        ),
    ]
}

/// **The app's stack** over [`app_files`]: each face followed by its
/// narrower instances.
fn app_stack(files: &[(Vec<u8>, FaceRole)]) -> (Vec<Face<'_>>, Vec<FaceRole>) {
    let (mut faces, mut roles) = (Vec::new(), Vec::new());
    for (data, role) in files {
        let face = Face::parse(data).expect("parse");
        let widths = baud_render::narrower_widths(&face, &[]);
        faces.push(face);
        roles.push(*role);
        for w in widths {
            let mut narrow = Face::parse(data).expect("parse");
            narrow.set_variations(&[baud_font::var::Variation {
                tag: baud_font::Tag::from_bytes(*b"wdth"),
                value: baud_font::var::Fixed::from_f32(w),
            }]);
            faces.push(narrow);
            roles.push(FaceRole::Narrower);
        }
    }
    assert!(
        roles.iter().filter(|r| **r == FaceRole::Narrower).count() >= 60,
        "the premise: every built face has its narrower instances"
    );
    (faces, roles)
}

/// **A word whose face changes between its syllables draws each syllable
/// in its own face, alike on every arm** (`[[source: ecma48]]` 8.3.117), on
/// the app's stack -- the built faces, each followed by its narrower
/// instances, behind the icon face: टर्मि and then नल in SGR 1 and back to
/// 0, under each count, अ then क्षर in SGR 1, and नल in SGR 3 and 1;3,
/// each the first frame of a renderer of its own (`first_frame`). Each in
/// bold draws as neither the word in its first syllable's face nor the
/// word in its last's; drawn whole in its first cell's face, the word drew
/// as the first. A change inside one syllable to the bold or bold italic
/// face of its first cell's family draws each glyph from its own cell's
/// face, as neither the syllable in its first face nor in its last; drawn
/// whole in its first cell's face, it drew as the first. A change between
/// upright and italic inside one draws the syllable in its first cell's
/// face, as the syllable written in one rendition draws, and a change of
/// colour alone draws, of the text or of its ground. And मल composed after
/// a bold क draws as the same letters written there.
#[test]
fn a_word_whose_face_changes_draws_alike_on_every_arm() {
    let files = app_files();
    let (faces, roles) = app_stack(&files);
    let frame = |text: &str, what: &str| {
        first_frame(
            (&faces, &roles),
            &format!("\x1b[?25l\x1b[2;3H{text}\x1b[m\x1b[4;1Hthe quick brown fox 0123456789"),
            (1, 2),
            what,
        )
        .0
    };
    for (what, mixed, first, last) in [
        ("bold after", "टर्मि\x1b[1mनल", "टर्मिनल", "\x1b[1mटर्मिनल"),
        (
            "regular after",
            "\x1b[1mटर्मि\x1b[mनल",
            "\x1b[1mटर्मिनल",
            "टर्मिनल",
        ),
        (
            "bold after under ?2027",
            "\x1b[?2027hटर्मि\x1b[1mनल",
            "\x1b[?2027hटर्मिनल",
            "\x1b[?2027h\x1b[1mटर्मिनल",
        ),
        ("bold after a vowel", "अ\x1b[1mक्षर", "अक्षर", "\x1b[1mअक्षर"),
    ] {
        let drawn = frame(mixed, what);
        assert!(
            drawn != frame(first, &format!("{what}: its first face")),
            "{what}: drawn in its first syllable's face"
        );
        assert!(
            drawn != frame(last, &format!("{what}: its last face")),
            "{what}: drawn in its last syllable's face"
        );
    }
    // The built Italic draws Devanagari upright, as the Regular's pixels:
    // each arm alike, and every glyph the cell arm draws the instance
    // arm's, from the same face (`first_frame`).
    for (what, mixed) in [
        ("italic after", "टर्मि\x1b[3mनल"),
        ("bold italic after", "टर्मि\x1b[1;3mनल"),
    ] {
        frame(mixed, what);
    }
    for (what, inside, first, last) in [
        (
            "bold inside a syllable",
            "टर्\x1b[1mमि\x1b[mनल",
            "टर्मिनल",
            "\x1b[1mटर्मिनल",
        ),
        (
            "regular inside a syllable",
            "\x1b[1mर्\x1b[mमि",
            "\x1b[1mर्मि",
            "र्मि",
        ),
        ("a bold sign", "म\x1b[1mि", "मि", "\x1b[1mमि"),
        (
            "bold italic inside a syllable",
            "\x1b[3mर्\x1b[1;3mमि",
            "\x1b[3mर्मि",
            "\x1b[1;3mर्मि",
        ),
    ] {
        let drawn = frame(inside, what);
        assert!(
            drawn != frame(first, &format!("{what}: its first face")),
            "{what}: drawn in its first cell's face"
        );
        assert!(
            drawn != frame(last, &format!("{what}: its last face")),
            "{what}: drawn in its last cell's face"
        );
    }
    for (what, inside, whole) in [
        ("a conjunct on its first cell", "क्\x1b[1mष", "क्ष"),
        ("italic inside a syllable", "र्\x1b[3mमि", "र्मि"),
        (
            "bold upright inside a syllable",
            "\x1b[3mर्\x1b[23;1mमि",
            "\x1b[3mर्मि",
        ),
    ] {
        assert_eq!(
            differing(
                &frame(inside, what),
                &frame(whole, &format!("{what}: whole"))
            ),
            0,
            "{what}: drawn as the syllable in its first cell's face"
        );
    }
    assert!(
        frame("टर्मि\x1b[31mनल", "red after") != frame("टर्मिनल", "red after: plain"),
        "the premise: the colour draws"
    );
    for (what, ground) in [
        ("a red ground after", "टर्मि\x1b[41mनल"),
        ("a green ground before", "\x1b[42mटर्मि\x1b[49mनल"),
    ] {
        assert!(
            frame(ground, what) != frame("टर्मिनल", &format!("{what}: plain")),
            "the premise: {what} draws"
        );
    }
    // **Composed**: मल composed after a bold क draws as the same letters
    // written there underlined, the क bold and मल regular; and मि composed
    // after a bold र्, inside the one syllable र्मि, the same, its glyphs
    // each from its own cell's face. Each on a renderer of its own.
    let failed = std::cell::RefCell::new(Vec::<String>::new());
    let fail = |what: String| failed.borrow_mut().push(what);
    for (taken, composed, what) in [
        ("\u{915}", "\u{92e}\u{932}", "a bold letter taken in"),
        (
            "\u{930}\u{94d}",
            "\u{92e}\u{93f}",
            "a bold part of the syllable taken in",
        ),
    ] {
        let renderer = Renderer::new(
            Stack::with_roles(&faces, &roles).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap();
        arms!(renderer => arms, t);
        composed_as_written(
            &mut arms,
            &mut t,
            (
                &format!(
                    "\x1bc\x1b[2 q\x1b[5;1HThe quick brown fox jumps over the lazy dog \
                     0123456789\x1b[3;1Hab\x1b[1m{taken}\x1b[m"
                ),
                composed,
            ),
            what,
            &three_arms,
            &fail,
        );
    }
    let failed = failed.into_inner();
    assert!(failed.is_empty(), "{failed:#?}");
}
